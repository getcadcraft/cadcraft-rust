//! geo-bool.js — booleans: openings (IfcRelVoidsElement), IfcBooleanResult / IfcBooleanClippingResult and half-spaces
//! (IfcHalfSpaceSolid, IfcBoxedHalfSpace, IfcPolygonalBoundedHalfSpace).
//!
//! Solids are cut by a boolean engine behind [`SolidOps`], shaped one-to-one on what the JS does with manifold-3d
//! (`new M.Mesh({numProp: 3, vertProperties: Float32Array, triVerts})`, `merge()`, `new M.Manifold(mesh)`,
//! `status()`, `subtract` / `add` / `intersect`, `getMesh()`). [`NoSolidOps`] is the JS without manifold-3d: a
//! half-space still cuts through [`clip_plane`] (exact plane clipping with capped holes), a convex opening is cut out of
//! the triangles, and a solid that cannot be subtracted is left whole with a note.

use std::any::Any;
use std::collections::HashMap;

use super::ctx::{GR, GeoCtx, GeoErr, bool_};
use super::curve::curve_points;
use super::math::{M4, V3, apply, cross, dot, invert, len, norm, perp, scale, sub, translation};
use super::mesh::Mesh;
use super::profile::Shape;
use super::solid::extrude_shapes;
use super::tri::plane_triangulate;
use crate::jsmath::{max, round};
use crate::model::Id;

/// An engine's solid (opaque to this crate).
pub struct Solid(pub Box<dyn Any>);

impl std::fmt::Debug for Solid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Solid")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Difference,
    Union,
    Intersection,
}

impl BoolOp {
    pub fn from_enum(s: &str) -> BoolOp {
        match s {
            "UNION" => BoolOp::Union,
            "INTERSECTION" => BoolOp::Intersection,
            _ => BoolOp::Difference,
        }
    }
}

/// A boolean engine, call for call what geo-bool.js asks of manifold-3d.
pub trait SolidOps: Sync {
    /// `hasManifold()`: an engine is loaded.
    fn loaded(&self) -> bool;
    /// `const mm = new M.Mesh({ numProp: 3, vertProperties, triVerts }); mm.merge(); new M.Manifold(mm)` with
    /// `status() === 'NoError'`; None when the constructor throws or the status is an error.
    #[allow(clippy::wrong_self_convention)] // the manifold-3d call it mirrors
    fn from_mesh(&self, vert_properties: &[f32], tri_verts: &[u32]) -> Option<Solid>;
    /// `a.subtract(b)`; None when it throws.
    fn subtract(&self, a: &Solid, b: &Solid) -> Option<Solid>;
    /// `a.add(b)`; None when it throws.
    fn add(&self, a: &Solid, b: &Solid) -> Option<Solid>;
    /// `a.intersect(b)`; None when it throws.
    fn intersect(&self, a: &Solid, b: &Solid) -> Option<Solid>;
    /// `getMesh()`: (vertProperties with numProp 3, triVerts).
    fn get_mesh(&self, s: &Solid) -> (Vec<f32>, Vec<u32>);
}

/// No boolean engine (the JS run without manifold-3d).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSolidOps;

impl SolidOps for NoSolidOps {
    fn loaded(&self) -> bool {
        false
    }
    fn from_mesh(&self, _: &[f32], _: &[u32]) -> Option<Solid> {
        None
    }
    fn subtract(&self, _: &Solid, _: &Solid) -> Option<Solid> {
        None
    }
    fn add(&self, _: &Solid, _: &Solid) -> Option<Solid> {
        None
    }
    fn intersect(&self, _: &Solid, _: &Solid) -> Option<Solid> {
        None
    }
    fn get_mesh(&self, _: &Solid) -> (Vec<f32>, Vec<u32>) {
        (Vec::new(), Vec::new())
    }
}

#[allow(clippy::cast_possible_truncation)] // Float32Array.from(pos): the rounding to f32 is the point
fn f32s(v: &[f64]) -> Vec<f32> {
    v.iter().map(|&x| x as f32).collect()
}

fn to_solid(ops: &dyn SolidOps, mesh: &Mesh) -> Option<Solid> {
    let (pos, idx) = mesh.welded(1e-6 * scale_of(mesh));
    if idx.is_empty() {
        return None;
    }
    ops.from_mesh(&f32s(&pos), &idx)
}

fn scale_of(mesh: &Mesh) -> f64 {
    let Some((lo, hi)) = mesh.bounds() else { return 1.0 };
    max(1e-9, max(max(hi[0] - lo[0], hi[1] - lo[1]), hi[2] - lo[2]))
}

fn from_solid(ops: &dyn SolidOps, s: &Solid) -> Mesh {
    let (vp, tv) = ops.get_mesh(s);
    let pos: Vec<f64> = vp.iter().map(|&x| f64::from(x)).collect();
    Mesh::from_welded(&pos, &tv)
}

fn centre_shift(m: &Mesh) -> (M4, M4) {
    let (lo, hi) = m.bounds().unwrap_or(([f64::NAN; 3], [f64::NAN; 3]));
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    (translation(-c[0], -c[1], -c[2]), translation(c[0], c[1], c[2]))
}

fn moved(m: &Mesh, t: &M4) -> Mesh {
    let mut o = Mesh::new();
    o.append(m, Some(t));
    o
}

fn joined(a: &Mesh, b: &Mesh) -> Mesh {
    let mut o = Mesh::new();
    o.append(a, None);
    o.append(b, None);
    o
}

/// a OP b, both meshes shifted near the origin first (float precision). None when it cannot.
pub fn boolean(ops: &dyn SolidOps, op: BoolOp, a: &Mesh, b: &Mesh, g: Option<&mut GeoCtx<'_>>) -> Option<Mesh> {
    if !ops.loaded() || a.is_empty() {
        return if op == BoolOp::Union { Some(joined(a, b)) } else { None };
    }
    if b.is_empty() {
        return Some(if op == BoolOp::Intersection { Mesh::new() } else { a.clone() });
    }
    let (t, tb) = centre_shift(a);
    let sa = to_solid(ops, &moved(a, &t))?;
    let sb = to_solid(ops, &moved(b, &t))?;
    let r = match op {
        BoolOp::Difference => ops.subtract(&sa, &sb),
        BoolOp::Union => ops.add(&sa, &sb),
        BoolOp::Intersection => ops.intersect(&sa, &sb),
    };
    match r {
        Some(r) => {
            let mut out = from_solid(ops, &r);
            out.transform(&tb);
            Some(out)
        }
        None => {
            if let Some(g) = g {
                g.note("A boolean could not be computed");
            }
            None
        }
    }
}

/// base minus every cutter (all in one frame). Never fails; a cutter that cannot be removed is noted and skipped.
pub fn subtract_all(ops: &dyn SolidOps, base: &Mesh, cutters: &[Mesh], g: &mut GeoCtx<'_>) -> Mesh {
    if cutters.is_empty() || base.is_empty() {
        return base.clone();
    }
    if ops.loaded() {
        let (t, tb) = centre_shift(base);
        if let Some(mut a) = to_solid(ops, &moved(base, &t)) {
            let mut rest: Vec<&Mesh> = Vec::new();
            for cut in cutters {
                let Some(b) = to_solid(ops, &moved(cut, &t)) else {
                    rest.push(cut);
                    continue;
                };
                match ops.subtract(&a, &b) {
                    Some(r) => a = r,
                    None => rest.push(cut),
                }
            }
            let mut out = from_solid(ops, &a);
            out.transform(&tb);
            for cut in rest {
                out = convex_cut(out, cut, g);
            }
            return out;
        }
    }
    let mut out = base.clone();
    for cut in cutters {
        out = convex_cut(out, cut, g);
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub n: V3,
    pub d: f64,
}

#[inline]
fn vtx(p: &[f64], k: u32) -> V3 {
    let i = usize::try_from(k).unwrap_or(usize::MAX).saturating_mul(3);
    [
        p.get(i).copied().unwrap_or(f64::NAN),
        p.get(i.saturating_add(1)).copied().unwrap_or(f64::NAN),
        p.get(i.saturating_add(2)).copied().unwrap_or(f64::NAN),
    ]
}

/// Outward face planes (n · p ≤ d inside) of a convex closed mesh, or None when it is not convex.
pub fn convex_planes(mesh: &Mesh, tol: f64) -> Option<Vec<Plane>> {
    let (lo, hi) = mesh.bounds()?;
    let size = max(1e-9, len(&sub(&hi, &lo)));
    let eps = size * tol;
    let mut planes: Vec<Plane> = Vec::new();
    for t in mesh.idx.as_chunks::<3>().0.iter() {
        let &[ia, ib, ic] = t;
        let (a, b, c) = (vtx(&mesh.pos, ia), vtx(&mesh.pos, ib), vtx(&mesh.pos, ic));
        let cr = cross(&sub(&b, &a), &sub(&c, &a));
        if len(&cr) < eps * eps {
            continue;
        }
        let n = norm(&cr);
        let d = dot(&n, &a);
        if planes.iter().any(|q| dot(&q.n, &n) > 1.0 - 1e-9 && (q.d - d).abs() < eps) {
            continue;
        }
        planes.push(Plane { n, d });
    }
    if planes.len() < 4 {
        return None;
    }
    for p in mesh.pos.as_chunks::<3>().0.iter() {
        let &[x, y, z] = p;
        for q in &planes {
            if q.n[0] * x + q.n[1] * y + q.n[2] * z - q.d > eps * 10.0 {
                return None;
            }
        }
    }
    Some(planes)
}

fn emit_fan(out: &mut Mesh, pts: &[V3], n: &V3) {
    if !out.room(pts.len()) {
        return;
    }
    let b0 = out.vertex_count();
    for p in pts {
        out.vertex(p, n);
    }
    let u = |k: usize| u32::try_from(b0.saturating_add(k)).unwrap_or(u32::MAX);
    for k in 1..pts.len().saturating_sub(1) {
        out.idx.extend_from_slice(&[u(0), u(k), u(k.saturating_add(1))]);
    }
}

/// base minus a convex cutter, on the triangles themselves (works on open or broken meshes; leaves the hole open).
fn convex_cut(base: Mesh, cutter: &Mesh, g: &mut GeoCtx<'_>) -> Mesh {
    let Some(planes) = convex_planes(cutter, 1e-6) else {
        g.note("An opening could not be cut (neither the element nor the opening is a closed convex solid)");
        return base;
    };
    let mut out = Mesh::new();
    for t in base.idx.as_chunks::<3>().0.iter() {
        let &[i0, i1, i2] = t;
        let mut poly = vec![vtx(&base.pos, i0), vtx(&base.pos, i1), vtx(&base.pos, i2)];
        let nrm = vtx(&base.nrm, i0);
        // quick reject: a triangle wholly outside one plane is kept as it is
        if planes.iter().any(|q| poly.iter().all(|p| dot(&q.n, p) - q.d >= 0.0)) {
            emit_fan(&mut out, &poly, &nrm);
            continue;
        }
        for q in &planes {
            let (front, back) = split_poly(&poly, q);
            if front.len() >= 3 {
                emit_fan(&mut out, &front, &nrm);
            }
            poly = back;
            if poly.len() < 3 {
                break;
            }
        }
    }
    out
}

/// Split a convex polygon by plane q: (part with n·p ≥ d, part with n·p < d).
fn split_poly(poly: &[V3], q: &Plane) -> (Vec<V3>, Vec<V3>) {
    let mut front = Vec::new();
    let mut back = Vec::new();
    let n = poly.len();
    for (i, a) in poly.iter().enumerate() {
        let b = poly.get(super::math::next(i, n)).unwrap_or(a);
        let da = dot(&q.n, a) - q.d;
        let db = dot(&q.n, b) - q.d;
        if da >= 0.0 {
            front.push(*a);
        } else {
            back.push(*a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            let x = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
            front.push(x);
            back.push(x);
        }
    }
    (front, back)
}

/// Keep the part of `mesh` on the side of the plane (point p, normal n) that n points to. Triangles are split exactly;
/// cut edges are chained into loops and capped (outer loops and the holes inside them) when they close.
pub fn clip_plane(mesh: &Mesh, p: &V3, n: &V3) -> Mesh {
    let n = norm(n);
    let mut out = Mesh::new();
    let sd: Vec<f64> =
        mesh.pos.as_chunks::<3>().0.iter().map(|q| (q[0] - p[0]) * n[0] + (q[1] - p[1]) * n[1] + (q[2] - p[2]) * n[2]).collect();
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &d in &sd {
        if d < lo {
            lo = d;
        }
        if d > hi {
            hi = d;
        }
    }
    let span = max(1e-9, hi - lo);
    let eps = span * 1e-9;
    if lo >= -eps {
        out.append(mesh, None);
        return out;
    }
    if hi <= eps {
        return out;
    }
    let mut segs: Vec<[V3; 2]> = Vec::new();
    let sdk = |k: u32| sd.get(usize::try_from(k).unwrap_or(usize::MAX)).copied().unwrap_or(f64::NAN);
    let v = |k: u32| vtx(&mesh.pos, k);
    let lerp = |i: u32, j: u32| {
        let t = sdk(i) / (sdk(i) - sdk(j));
        let (a, b) = (v(i), v(j));
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    };
    for t in mesh.idx.as_chunks::<3>().0.iter() {
        let &[i0, i1, i2] = t;
        let ids = [i0, i1, i2];
        let k = ids.iter().filter(|&&i| sdk(i) >= -eps).count();
        let fnrm = vtx(&mesh.nrm, i0);
        if k == 3 {
            emit_fan(&mut out, &[v(i0), v(i1), v(i2)], &fnrm);
            continue;
        }
        if k == 0 {
            continue;
        }
        // walk the triangle's edges, keeping the inside part
        let mut poly = Vec::new();
        let mut cut: Vec<V3> = Vec::new();
        for (e, &i) in ids.iter().enumerate() {
            let j = ids.get(super::math::next(e, 3)).copied().unwrap_or(i);
            let (ii, jj) = (sdk(i) >= -eps, sdk(j) >= -eps);
            if ii {
                poly.push(v(i));
            }
            if ii != jj && sdk(i).abs() > eps && sdk(j).abs() > eps {
                let q = lerp(i, j);
                poly.push(q);
                cut.push(q);
            } else if ii && !jj && sdk(i).abs() <= eps {
                cut.push(v(i));
            } else if !ii && jj && sdk(j).abs() <= eps {
                cut.push(v(j));
            }
        }
        if poly.len() >= 3 {
            emit_fan(&mut out, &poly, &fnrm);
        }
        if let [a, b] = cut.as_slice() {
            segs.push([*a, *b]);
        }
    }
    cap_loops(&mut out, &segs, &scale(&n, -1.0), eps * 10.0 + span * 1e-7);
    out
}

/// A grid key component: `String(Math.round(x / tol))` (−0 and 0 alike; every NaN alike).
fn key_part(x: f64) -> u64 {
    let r = round(x);
    if r.is_nan() {
        u64::MAX
    } else if r == 0.0 {
        0
    } else {
        r.to_bits()
    }
}

/// Chain cut segments into closed loops on the plane and fill them (normal `cn`, facing out of the kept part).
fn cap_loops(out: &mut Mesh, segs: &[[V3; 2]], cn: &V3, tol: f64) {
    if segs.is_empty() {
        return;
    }
    let key = |q: &V3| (key_part(q[0] / tol), key_part(q[1] / tol), key_part(q[2] / tol));
    // the JS Map in insertion order: nodes by first appearance
    let mut index: HashMap<(u64, u64, u64), usize> = HashMap::new();
    let mut nodes: Vec<(V3, Vec<usize>)> = Vec::new();
    for [a, b] in segs {
        let (ka, kb) = (key(a), key(b));
        if ka == kb {
            continue;
        }
        let ia = *index.entry(ka).or_insert_with(|| {
            nodes.push((*a, Vec::new()));
            nodes.len().saturating_sub(1)
        });
        let ib = *index.entry(kb).or_insert_with(|| {
            nodes.push((*b, Vec::new()));
            nodes.len().saturating_sub(1)
        });
        if let Some(n) = nodes.get_mut(ia) {
            n.1.push(ib);
        }
        if let Some(n) = nodes.get_mut(ib) {
            n.1.push(ia);
        }
    }
    let ek = |a: usize, b: usize| if a < b { (a, b) } else { (b, a) };
    let mut used: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    let mut loops: Vec<Vec<V3>> = Vec::new();
    for start in 0..nodes.len() {
        let tos = nodes.get(start).map(|n| n.1.clone()).unwrap_or_default();
        for first in tos {
            if used.contains(&ek(start, first)) {
                continue;
            }
            let mut lp = vec![nodes.get(start).map_or([f64::NAN; 3], |n| n.0)];
            used.insert(ek(start, first));
            let (mut prev, mut cur) = (start, first);
            let mut guard = 0u32;
            while cur != start && guard < 1_000_000 {
                guard = guard.saturating_add(1);
                let Some(node) = nodes.get(cur) else { break };
                lp.push(node.0);
                let nxt = node.1.iter().copied().find(|&x| x != prev && !used.contains(&ek(cur, x)));
                let Some(nxt) = nxt else { break };
                used.insert(ek(cur, nxt));
                prev = cur;
                cur = nxt;
            }
            if cur == start && lp.len() >= 3 {
                loops.push(lp);
            }
        }
    }
    if loops.is_empty() {
        return;
    }
    // 2D frame on the plane; loops CCW about cn are outers, nesting decides holes
    let u = perp(cn);
    let w = cross(cn, &u);
    struct L {
        l: Vec<V3>,
        p2: Vec<[f64; 2]>,
        area: f64,
        ccw: bool,
    }
    let mut ls: Vec<L> = loops
        .into_iter()
        .map(|l| {
            let p2: Vec<[f64; 2]> = l.iter().map(|q| [dot(q, &u), dot(q, &w)]).collect();
            let mut a = 0.0;
            let n = p2.len();
            for (i, x) in p2.iter().enumerate() {
                let y = p2.get(super::math::next(i, n)).unwrap_or(x);
                a += x[0] * y[1] - y[0] * x[1];
            }
            L { l, p2, area: (a / 2.0).abs(), ccw: a > 0.0 }
        })
        .collect();
    ls.sort_by(|x, y| {
        let d = y.area - x.area;
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let inside = |pt: &[f64; 2], poly: &[[f64; 2]]| {
        let mut c = false;
        let n = poly.len();
        let mut j = n.wrapping_sub(1);
        for i in 0..n {
            if let (Some(a), Some(b)) = (poly.get(i), poly.get(j))
                && (a[1] > pt[1]) != (b[1] > pt[1])
                && pt[0] < (b[0] - a[0]) * (pt[1] - a[1]) / (b[1] - a[1]) + a[0]
            {
                c = !c;
            }
            j = i;
        }
        c
    };
    let first = |x: &L| x.p2.first().copied().unwrap_or([f64::NAN; 2]);
    let depth: Vec<usize> = ls.iter().enumerate().map(|(i, x)| ls.iter().take(i).filter(|y| inside(&first(x), &y.p2)).count()).collect();
    for (i, li) in ls.iter().enumerate() {
        let di = depth.get(i).copied().unwrap_or(0);
        if di % 2 == 1 {
            continue;
        }
        let mut outer = li.l.clone();
        if !li.ccw {
            outer.reverse();
        }
        let mut rings = vec![outer];
        for (j, lj) in ls.iter().enumerate().skip(i.saturating_add(1)) {
            if depth.get(j).copied() == Some(di.saturating_add(1)) && inside(&first(lj), &li.p2) {
                let mut h = lj.l.clone();
                if lj.ccw {
                    h.reverse();
                }
                rings.push(h);
            }
        }
        let tris = plane_triangulate(&rings);
        let total: usize = rings.iter().map(Vec::len).sum();
        if !out.room(total) {
            continue;
        }
        let base = out.vertex_count();
        for r in &rings {
            for q in r {
                out.vertex(q, cn);
            }
        }
        for t in tris.as_chunks::<3>().0.iter() {
            out.idx.extend(t.iter().map(|&k| u32::try_from(base.saturating_add(k)).unwrap_or(u32::MAX)));
        }
    }
}

/// The plane of an IfcHalfSpaceSolid's BaseSurface (IfcPlane) and the side its material is on.
fn half_space_plane(g: &mut GeoCtx<'_>, hs: Option<Id>) -> GR<(V3, V3)> {
    let surf = g.ra(hs, "BaseSurface")?;
    let pos = g.axis2(g.ra(surf, "Position")?)?;
    let p = [pos[12], pos[13], pos[14]];
    let n = norm(&[pos[8], pos[9], pos[10]]);
    // AgreementFlag TRUE: the plane's normal points away from the half-space's material
    let agree = bool_(g.a(hs, "AgreementFlag")?, Some(true)).unwrap_or(true);
    Ok((p, if agree { n } else { scale(&n, -1.0) }))
}

/// base − half-space solid (the half-space's material is behind its outward normal n: keep what n points to).
pub fn half_space_cut(ops: &dyn SolidOps, g: &mut GeoCtx<'_>, base: Mesh, hs: Option<Id>) -> GR<Mesh> {
    let t = g.t(hs);
    let bs = g.ra(hs, "BaseSurface")?;
    if g.t(bs) != Some("IFCPLANE") {
        g.note("Half-spaces on curved surfaces are not cut yet");
        return Ok(base);
    }
    let (p, n) = half_space_plane(g, hs)?;
    if t == Some("IFCHALFSPACESOLID") || t == Some("IFCBOXEDHALFSPACE") {
        return Ok(clip_plane(&base, &p, &n));
    }
    if t == Some("IFCPOLYGONALBOUNDEDHALFSPACE") {
        let pm = g.axis2(g.ra(hs, "Position")?)?;
        let pb = g.ra(hs, "PolygonalBoundary")?;
        let mut ring: Vec<[f64; 2]> = curve_points(g, pb, 0)?.iter().map(|q| [q[0], q[1]]).collect();
        if ring.len() > 1
            && let (Some(a), Some(b)) = (ring.first(), ring.last())
            && (a[0] - b[0]).abs() < 1e-9
            && (a[1] - b[1]).abs() < 1e-9
        {
            ring.pop();
        }
        if ring.len() < 3 {
            return Ok(clip_plane(&base, &p, &n));
        }
        let l = match base.bounds() {
            Some((lo, hi)) => 4.0 * max(1.0, len(&sub(&hi, &lo))) + len(&sub(&[pm[12], pm[13], pm[14]], &lo)),
            None => 1e6,
        };
        // the bounded half-space as a closed solid: the prism over the boundary, long enough, cut by the plane
        let mut prism = extrude_shapes(&[Shape { outer: ring.clone(), holes: vec![], open: false }], &[0.0, 0.0, 1.0], 2.0 * l, None);
        prism.transform(&translation(0.0, 0.0, -l));
        prism.transform(&pm);
        let solid = clip_plane(&prism, &p, &scale(&n, -1.0));
        if let Some(r) = boolean(ops, BoolOp::Difference, &base, &solid, Some(g)) {
            return Ok(r);
        }
        // not a closed manifold: cut by the plane only where the triangles lie over the boundary
        let pi = invert(&pm);
        let cut = clip_plane(&base, &p, &n);
        if pi.is_none() && (!base.idx.is_empty() || !cut.idx.is_empty()) {
            return Err(super::ctx::type_error_read("null", "0"));
        }
        let pi = pi.unwrap_or([f64::NAN; 16]);
        let in_prism = |q: &V3| {
            let lp = apply(&pi, q[0], q[1], q[2]);
            let mut c = false;
            let n = ring.len();
            let mut j = n.wrapping_sub(1);
            for i in 0..n {
                if let (Some(a), Some(b)) = (ring.get(i), ring.get(j))
                    && (a[1] > lp[1]) != (b[1] > lp[1])
                    && lp[0] < (b[0] - a[0]) * (lp[1] - a[1]) / (b[1] - a[1]) + a[0]
                {
                    c = !c;
                }
                j = i;
            }
            c
        };
        let mut keep_out = Mesh::new();
        // keep triangles outside the boundary from the uncut mesh, inside it from the cut one
        split_by(&base, &|c| !in_prism(c), &mut keep_out);
        split_by(&cut, &|c| in_prism(c), &mut keep_out);
        g.note("A bounded half-space was cut approximately (the mesh is not closed)");
        return Ok(keep_out);
    }
    let pr = t.map_or_else(|| "null".to_string(), |t| g.pretty(t));
    g.note(format!("{pr} is not cut yet"));
    Ok(base)
}

fn split_by(mesh: &Mesh, test: &dyn Fn(&V3) -> bool, into: &mut Mesh) {
    for t in mesh.idx.as_chunks::<3>().0.iter() {
        let &[a, b, c] = t;
        let (pa, pb, pc) = (vtx(&mesh.pos, a), vtx(&mesh.pos, b), vtx(&mesh.pos, c));
        let cen = [(pa[0] + pb[0] + pc[0]) / 3.0, (pa[1] + pb[1] + pc[1]) / 3.0, (pa[2] + pb[2] + pc[2]) / 3.0];
        if !test(&cen) || !into.room(3) {
            continue;
        }
        let base = into.vertex_count();
        for k in [a, b, c] {
            into.vertex(&vtx(&mesh.pos, k), &vtx(&mesh.nrm, k));
        }
        let u = |k: usize| u32::try_from(base.saturating_add(k)).unwrap_or(u32::MAX);
        into.idx.extend_from_slice(&[u(0), u(1), u(2)]);
    }
}

/// An error to report when a boolean engine misbehaves (unused by NoSolidOps).
pub fn engine_error(msg: &str) -> GeoErr {
    GeoErr(msg.to_string())
}
