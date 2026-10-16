//! geo-solid.js — swept solids and primitives as meshes, each in its own Position frame (IFC4 ADD2 TC1 §8.8):
//! IfcExtrudedAreaSolid (+ Tapered), IfcRevolvedAreaSolid (+ Tapered), IfcSweptDiskSolid (+ Polygonal),
//! IfcSurfaceCurveSweptAreaSolid, IfcFixedReferenceSweptAreaSolid, IfcDirectrixCurveSweptAreaSolid, and the CSG
//! primitives IfcBlock, IfcRightCircularCylinder, IfcRightCircularCone, IfcSphere, IfcRectangularPyramid.

use super::align::{is_segmented, point_along, segmented_curve};
use super::ctx::{GR, GeoCtx, num};
use super::curve::{arc_segments, count, curve_points};
use super::math::{M4, V2, V3, add, apply, apply_v, cross, dot, is_zero, len, norm, perp, rotation, scale, sub};
use super::mesh::Mesh;
use super::profile::{Shape, profile_shapes};
use crate::jsmath::{JsMath, max, sign};
use crate::model::Id;
use crate::util::usize_f64;

const TAU: f64 = 2.0 * std::f64::consts::PI;

#[inline]
fn p3(p: &V2, z: f64) -> V3 {
    [p[0], p[1], z]
}

fn rev<T: Clone>(v: &[T]) -> Vec<T> {
    let mut r = v.to_vec();
    r.reverse();
    r
}

/// Extrude 2D shapes (z = 0) along dir × depth. Faces point outward whatever the direction's sign.
pub fn extrude_shapes(shapes: &[Shape], dir: &V3, depth: f64, end_shapes: Option<&[Shape]>) -> Mesh {
    let mut m = Mesh::new();
    let d = scale(dir, depth);
    let up = d[2] >= 0.0; // shapes are counter-clockwise seen from +z
    for (si, s) in shapes.iter().enumerate() {
        if s.open {
            continue;
        }
        let e = match end_shapes.and_then(|es| es.get(si)) {
            Some(es) if same_counts(s, es) => es,
            _ => s,
        };
        let mut bottom: Vec<Vec<V3>> = vec![s.outer.iter().map(|p| p3(p, 0.0)).collect()];
        bottom.extend(s.holes.iter().map(|h| h.iter().map(|p| p3(p, 0.0)).collect()));
        let mut top: Vec<Vec<V3>> = vec![e.outer.iter().map(|p| add(&p3(p, 0.0), &d)).collect()];
        top.extend(e.holes.iter().map(|h| h.iter().map(|p| add(&p3(p, 0.0), &d)).collect()));
        // bottom faces down (reversed), top faces up — swapped when extruding downward
        if up {
            m.polygon(&bottom.iter().map(|r| rev(r)).collect::<Vec<_>>());
            m.polygon(&top);
        } else {
            m.polygon(&bottom);
            m.polygon(&top.iter().map(|r| rev(r)).collect::<Vec<_>>());
        }
        for (a, b) in bottom.iter().zip(top.iter()) {
            let n = a.len();
            for i in 0..n {
                let j = super::math::next(i, n);
                let (Some(&ai), Some(&aj), Some(&bj), Some(&bi)) = (a.get(i), a.get(j), b.get(j), b.get(i)) else { continue };
                let q = [ai, aj, bj, bi];
                if up {
                    quad(&mut m, &q);
                } else {
                    quad(&mut m, &[bi, bj, aj, ai]);
                }
            }
        }
    }
    m
}

fn same_counts(a: &Shape, b: &Shape) -> bool {
    a.outer.len() == b.outer.len() && a.holes.len() == b.holes.len() && a.holes.iter().zip(b.holes.iter()).all(|(h, k)| h.len() == k.len())
}

/// A planar or slightly twisted quad as two triangles (one polygon when planar).
pub fn quad(m: &mut Mesh, q: &[V3; 4]) {
    let [a, b, c, d] = q;
    if len(&sub(a, b)) < 1e-12 || len(&sub(c, d)) < 1e-12 {
        m.tri(a, c, d);
        return;
    }
    let n1 = cross(&sub(b, a), &sub(c, a));
    let n2 = cross(&sub(c, a), &sub(d, a));
    if dot(&norm(&n1), &norm(&n2)) > 0.9999 {
        m.polygon(&[q.to_vec()]);
    } else {
        m.tri(a, b, c);
        m.tri(a, c, d);
    }
}

pub fn extruded(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let sa = g.ra(id, "SweptArea")?;
    let shapes = profile_shapes(g, sa, 0)?;
    let dir = g.dirv(g.ra(id, "ExtrudedDirection")?, [0.0, 0.0, 1.0])?;
    let depth = g.na(id, "Depth", 0.0)?;
    let end = if g.t(id) == Some("IFCEXTRUDEDAREASOLIDTAPERED") {
        let ea = g.ra(id, "EndSweptArea")?;
        Some(profile_shapes(g, ea, 0)?)
    } else {
        None
    };
    let mut m = extrude_shapes(&shapes, &dir, depth, end.as_deref());
    let pos = g.axis2(g.ra(id, "Position")?)?;
    m.transform(&pos);
    Ok(m)
}

/// IfcRevolvedAreaSolid: the profile turned about Axis (in the profile plane) by Angle.
pub fn revolved(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let sa = g.ra(id, "SweptArea")?;
    let shapes = profile_shapes(g, sa, 0)?;
    let ax = g.ra(id, "Axis")?;
    let o = g.point(g.ra(ax, "Location")?)?;
    let axis = g.dirv(g.ra(ax, "Axis")?, [0.0, 1.0, 0.0])?;
    let mut angle = g.na(id, "Angle", 0.0)? * g.angle_unit;
    let full = angle.abs() >= TAU - 1e-9;
    if full {
        angle = TAU * sign(super::math::or(angle, 1.0));
    }
    let steps = arc_segments(angle.abs(), 1.0);
    let nsteps = count(steps)?;
    let rots: Vec<M4> = (0..=nsteps).map(|k| rotation(&axis, angle * usize_f64(k) / steps)).collect();
    let rot = |k: usize, p: &V2| -> V3 {
        let r = rots.get(k).copied().unwrap_or([f64::NAN; 16]);
        add(&o, &apply_v(&r, &sub(&p3(p, 0.0), &o)))
    };
    let mut m = Mesh::new();
    for sh in &shapes {
        if sh.open {
            continue;
        }
        let mut rings: Vec<&Vec<V2>> = vec![&sh.outer];
        rings.extend(sh.holes.iter());
        // which way the profile leaves its plane: points move along sign(angle) × axis × (p − o); the profile is
        // counter-clockwise about +z, so that sign fixes the winding of the sides and caps
        let nl = usize_f64(sh.outer.len());
        let c = sh.outer.iter().fold([0.0, 0.0], |acc, p| [acc[0] + p[0] / nl, acc[1] + p[1] / nl]);
        let s = if sign(angle) * cross(&axis, &sub(&p3(&c, 0.0), &o))[2] < 0.0 { -1.0 } else { 1.0 };
        for ring in &rings {
            let n = ring.len();
            for k in 0..nsteps {
                for i in 0..n {
                    let j = super::math::next(i, n);
                    let (Some(pi), Some(pj)) = (ring.get(i), ring.get(j)) else { continue };
                    let q = [rot(k, pi), rot(k, pj), rot(k.saturating_add(1), pj), rot(k.saturating_add(1), pi)];
                    if s < 0.0 {
                        quad(&mut m, &[q[3], q[2], q[1], q[0]]);
                    } else {
                        quad(&mut m, &q);
                    }
                }
            }
        }
        if !full {
            let start: Vec<Vec<V3>> = rings.iter().map(|r| r.iter().map(|p| rot(0, p)).collect()).collect();
            let end: Vec<Vec<V3>> = rings.iter().map(|r| r.iter().map(|p| rot(nsteps, p)).collect()).collect();
            if s < 0.0 {
                m.polygon(&start);
                m.polygon(&end.iter().map(|r| rev(r)).collect::<Vec<_>>());
            } else {
                m.polygon(&start.iter().map(|r| rev(r)).collect::<Vec<_>>());
                m.polygon(&end);
            }
        }
    }
    let pos = g.axis2(g.ra(id, "Position")?)?;
    m.transform(&pos);
    Ok(m)
}

/// One frame along a path.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub p: V3,
    pub t: V3,
    pub n: V3,
    pub b: V3,
    pub mit: f64,
    pub tin: Option<V3>,
    pub tout: Option<V3>,
}

/// Frames along a 3D polyline by parallel transport.
pub fn frames(path: &[V3]) -> Vec<Frame> {
    let mut out: Vec<Frame> = Vec::with_capacity(path.len());
    let n = path.len();
    let (Some(p0), Some(p1)) = (path.first(), path.get(1)) else { return out };
    let t0 = norm(&sub(p1, p0));
    let mut nrm = perp(&t0);
    for (i, p) in path.iter().enumerate() {
        let tin = if i > 0 { path.get(i.saturating_sub(1)).map(|q| norm(&sub(p, q))) } else { None };
        let tout = if i.saturating_add(1) < n { path.get(i.saturating_add(1)).map(|q| norm(&sub(q, p))) } else { None };
        let mut t = match (tin, tout) {
            (Some(a), Some(b)) => norm(&add(&a, &b)),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => [0.0; 3],
        };
        if is_zero(&t) {
            t = tout.or(tin).unwrap_or(t);
        }
        // transport the normal from the previous tangent to this one
        if let Some(prev) = out.last() {
            nrm = transport(&nrm, &prev.t, &t);
        } else {
            nrm = norm(&sub(&nrm, &scale(&t, dot(&nrm, &t))));
        }
        let b = cross(&t, &nrm);
        // mitre: scale the section across the bend so the wall keeps its thickness
        let mit = match (tin, tout) {
            (Some(a), Some(_)) => 1.0 / max(0.25, dot(&a, &t)),
            _ => 1.0,
        };
        out.push(Frame { p: *p, t, n: nrm, b, mit, tin, tout });
    }
    out
}

fn transport(v: &V3, a: &V3, b: &V3) -> V3 {
    let ax = cross(a, b);
    let s = len(&ax);
    if s < 1e-12 {
        return norm(&sub(v, &scale(b, dot(v, b))));
    }
    let ang = s.jatan2(dot(a, b));
    let r = rotation(&ax, ang);
    let w = apply(&r, v[0], v[1], v[2]);
    norm(&sub(&w, &scale(b, dot(&w, b))))
}

/// A ring of 2D section points placed at frame f (section x along f.n, y along f.b), mitred across the bend.
fn place(f: &Frame, ring: &[V2]) -> Vec<V3> {
    ring.iter()
        .map(|&[x, y]| {
            let mut off = add(&scale(&f.n, x), &scale(&f.b, y));
            if f.mit != 1.0
                && let (Some(tin), Some(tout)) = (f.tin, f.tout)
            {
                // stretch the component along the bend plane so the section lies on the mitre plane
                let bend = norm(&sub(&tout, &tin));
                let k = dot(&off, &bend);
                off = add(&off, &scale(&bend, k * (f.mit - 1.0)));
            }
            add(&f.p, &off)
        })
        .collect()
}

/// Sweep closed 2D rings (outer CCW, holes CW) along frames; caps at the ends.
fn sweep_framed(fr: &[Frame], shapes: &[Shape]) -> Mesh {
    let mut m = Mesh::new();
    if fr.len() < 2 {
        return m;
    }
    for s in shapes {
        if s.open {
            continue;
        }
        let mut rings: Vec<&Vec<V2>> = vec![&s.outer];
        rings.extend(s.holes.iter());
        let placed: Vec<Vec<Vec<V3>>> = fr.iter().map(|f| rings.iter().map(|r| place(f, r)).collect()).collect();
        for r in 0..rings.len() {
            let n = rings.get(r).map_or(0, |x| x.len());
            for w in placed.windows(2) {
                let [pa, pb] = w else { continue };
                let (Some(a), Some(b)) = (pa.get(r), pb.get(r)) else { continue };
                for i in 0..n {
                    let j = super::math::next(i, n);
                    let (Some(&ai), Some(&aj), Some(&bj), Some(&bi)) = (a.get(i), a.get(j), b.get(j), b.get(i)) else { continue };
                    quad(&mut m, &[ai, aj, bj, bi]);
                }
            }
        }
        // the section is CCW about +t (n × b = t): the start cap faces back, the end cap forward
        if let Some(first) = placed.first() {
            m.polygon(&first.iter().map(|r| rev(r)).collect::<Vec<_>>());
        }
        if let Some(last) = placed.last() {
            m.polygon(last);
        }
    }
    if m.volume() < 0.0 {
        m.flip();
    }
    m
}

/// Sweep closed 2D rings along a path (parallel-transport frames).
pub fn sweep_rings(path: &[V3], shapes: &[Shape]) -> Mesh {
    sweep_framed(&frames(path), shapes)
}

fn circle_ring(rad: f64) -> GR<Vec<V2>> {
    let n = arc_segments(TAU, rad);
    let cnt = count(n)?;
    Ok((0..cnt)
        .map(|i| {
            let a = TAU * usize_f64(i) / n;
            [rad * a.jcos(), rad * a.jsin()]
        })
        .collect())
}

/// IfcSweptDiskSolid (and the polygonal variant with bends): a tube along the directrix.
pub fn swept_disk(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let d = g.ra(id, "Directrix")?;
    let mut path = dedupe(&curve_points(g, d, 0)?);
    let r = g.na(id, "Radius", 0.0)?;
    let ri = g.na(id, "InnerRadius", 0.0)?;
    let bend = g.na(id, "FilletRadius", 0.0)?;
    if bend > 0.0 && path.len() > 2 {
        path = fillet_path(&path, bend)?;
    }
    let holes = if ri > 0.0 && ri < r { vec![rev(&circle_ring(ri)?)] } else { Vec::new() };
    Ok(sweep_rings(&path, &[Shape { outer: circle_ring(r)?, holes, open: false }]))
}

fn dedupe(path: &[V3]) -> Vec<V3> {
    let mut out: Vec<V3> = Vec::new();
    for p in path {
        if out.last().is_none_or(|l| len(&sub(l, p)) > 1e-9) {
            out.push(*p);
        }
    }
    out
}

/// Round the corners of a 3D polyline with arcs of radius r (IfcSweptDiskSolidPolygonal bends).
pub fn fillet_path(path: &[V3], r: f64) -> GR<Vec<V3>> {
    let mut out: Vec<V3> = path.first().copied().into_iter().collect();
    let pi = std::f64::consts::PI;
    for w in path.windows(3) {
        let [pp, p, pn] = w else { continue };
        let a = norm(&sub(pp, p));
        let b = norm(&sub(pn, p));
        let cos_t = dot(&a, &b);
        let half = max(-1.0, crate::jsmath::min(1.0, cos_t)).jacos() / 2.0;
        if half < 1e-6 || half > pi / 2.0 - 1e-6 {
            out.push(*p);
            continue;
        }
        let d = crate::jsmath::min(crate::jsmath::min(r / half.jtan(), len(&sub(pp, p)) * 0.999), len(&sub(pn, p)) * 0.999);
        let rr = d * half.jtan();
        let s = add(p, &scale(&a, d));
        let e = add(p, &scale(&b, d));
        let c = add(p, &scale(&norm(&add(&a, &b)), rr / half.jsin()));
        let u = norm(&sub(&s, &c));
        let wv = norm(&sub(&e, &c));
        let sweep = max(-1.0, crate::jsmath::min(1.0, dot(&u, &wv))).jacos();
        let ax = norm(&cross(&u, &wv));
        let n = max(2.0, arc_segments(sweep, rr));
        for k in 0..count(n + 1.0)? {
            let rm = rotation(&ax, sweep * usize_f64(k) / n);
            out.push(add(&c, &scale(&apply(&rm, u[0], u[1], u[2]), rr)));
        }
    }
    if let Some(l) = path.last() {
        out.push(*l);
    }
    Ok(out)
}

/// Profile swept along a directrix: IfcSurfaceCurveSweptAreaSolid, IfcFixedReferenceSweptAreaSolid,
/// IfcDirectrixCurveSweptAreaSolid.
pub fn swept_area(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let sa = g.ra(id, "SweptArea")?;
    let shapes = profile_shapes(g, sa, 0)?;
    let dir_id = g.ra(id, "Directrix")?;
    let mut path = dedupe(&curve_points(g, dir_id, 0)?);
    // an alignment directrix is cut to StartParam … EndParam (distances along it)
    let sp = num(g.a(id, "StartParam")?);
    let ep = num(g.a(id, "EndParam")?);
    if is_segmented(g, dir_id)? && (sp.is_some() || ep.is_some()) {
        let c = segmented_curve(g, dir_id)?;
        let d0 = sp.unwrap_or(0.0);
        let d1 = ep.unwrap_or_else(|| c.dist.last().copied().unwrap_or(f64::NAN));
        let mut pts = vec![point_along(g, dir_id, d0)?.0];
        pts.extend(c.pts.iter().enumerate().filter(|(i, _)| c.dist.get(*i).is_some_and(|&d| d > d0 && d < d1)).map(|(_, p)| *p));
        pts.push(point_along(g, dir_id, d1)?.0);
        path = dedupe(&pts);
    }
    let t = g.t(id);
    let m = if t == Some("IFCFIXEDREFERENCESWEPTAREASOLID")
        || t == Some("IFCDIRECTRIXCURVESWEPTAREASOLID")
        || t == Some("IFCSURFACECURVESWEPTAREASOLID")
    {
        // keep the profile's x axis toward the fixed reference — for a curve on a surface, the surface's normal (a plane's
        // axis; other surfaces: their position's z) — and y along tangent × x
        let mut rf = g.dirv(g.ra(id, "FixedReference")?, [0.0, 0.0, 1.0])?;
        if t == Some("IFCSURFACECURVESWEPTAREASOLID") {
            let rs = g.ra(id, "ReferenceSurface")?;
            let pos = if rs.is_some() { g.ra(rs, "Position")? } else { None };
            if pos.is_some() {
                let mm = g.axis2(pos)?;
                rf = [mm[8], mm[9], mm[10]];
            }
        }
        let fr: Vec<Frame> = frames(&path)
            .into_iter()
            .map(|f| {
                let mut x = sub(&rf, &scale(&f.t, dot(&rf, &f.t)));
                if len(&x) < 1e-9 {
                    x = f.n;
                }
                let x = norm(&x);
                Frame { n: x, b: cross(&f.t, &x), ..f }
            })
            .collect();
        sweep_framed(&fr, &shapes)
    } else {
        sweep_rings(&path, &shapes)
    };
    let mut m = m;
    let pos = g.axis2(g.ra(id, "Position")?)?;
    m.transform(&pos);
    Ok(m)
}

/// CSG primitives (IfcBlock, IfcRightCircularCylinder, IfcRightCircularCone, IfcSphere, IfcRectangularPyramid).
pub fn primitive(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let t = g.t(id).unwrap_or("");
    let n = |g: &GeoCtx<'_>, name: &str| g.na(id, name, 0.0);
    let mut m = Mesh::new();
    match t {
        "IFCBLOCK" => {
            let (x, y, z) = (n(g, "XLength")?, n(g, "YLength")?, n(g, "ZLength")?);
            m = extrude_shapes(
                &[Shape { outer: vec![[0.0, 0.0], [x, 0.0], [x, y], [0.0, y]], holes: vec![], open: false }],
                &[0.0, 0.0, 1.0],
                z,
                None,
            );
        }
        "IFCRIGHTCIRCULARCYLINDER" => {
            let (r, h) = (n(g, "Radius")?, n(g, "Height")?);
            m = extrude_shapes(&[Shape { outer: circle_ring(r)?, holes: vec![], open: false }], &[0.0, 0.0, 1.0], h, None);
        }
        "IFCRIGHTCIRCULARCONE" => {
            let (r, h) = (n(g, "BottomRadius")?, n(g, "Height")?);
            let ring: Vec<V3> = circle_ring(r)?.iter().map(|p| p3(p, 0.0)).collect();
            m.polygon(&[rev(&ring)]);
            let k = ring.len();
            for i in 0..k {
                if let (Some(a), Some(b)) = (ring.get(i), ring.get(super::math::next(i, k))) {
                    m.tri(a, b, &[0.0, 0.0, h]);
                }
            }
        }
        "IFCRECTANGULARPYRAMID" => {
            let (x, y, h) = (n(g, "XLength")?, n(g, "YLength")?, n(g, "Height")?);
            let b = [[0.0, 0.0, 0.0], [x, 0.0, 0.0], [x, y, 0.0], [0.0, y, 0.0]];
            let top = [x / 2.0, y / 2.0, h];
            m.polygon(&[rev(&b)]);
            for i in 0..4 {
                if let (Some(p), Some(q)) = (b.get(i), b.get(super::math::next(i, 4))) {
                    m.tri(p, q, &top);
                }
            }
        }
        "IFCSPHERE" => {
            let r = n(g, "Radius")?;
            let (st, sl) = (12usize, 24usize);
            let pi = std::f64::consts::PI;
            let pt = |i: usize, j: usize| -> V3 {
                let th = pi * usize_f64(i) / usize_f64(st);
                let ph = TAU * usize_f64(j) / usize_f64(sl);
                [r * th.jsin() * ph.jcos(), r * th.jsin() * ph.jsin(), r * th.jcos()]
            };
            for i in 0..st {
                for j in 0..sl {
                    let (a, b, c, d) =
                        (pt(i, j), pt(i.saturating_add(1), j), pt(i.saturating_add(1), j.saturating_add(1)), pt(i, j.saturating_add(1)));
                    if i > 0 {
                        m.tri(&a, &b, &d);
                    }
                    if i.saturating_add(1) < st {
                        m.tri(&b, &c, &d);
                    }
                }
            }
        }
        other => {
            let p = g.pretty(other);
            g.note(format!("{p} is not drawn yet"));
            return Ok(m);
        }
    }
    let pos = g.axis2(g.ra(id, "Position")?)?;
    m.transform(&pos);
    Ok(m)
}
