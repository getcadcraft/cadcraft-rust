//! geo-nurbs.js — faces on curved surfaces: IfcAdvancedBrep (+ WithVoids), and IfcAdvancedFace / IfcFaceSurface
//! wherever they appear. Written from IFC4 ADD2 TC1 §8.9 (surfaces), §8.19 (topology) and the B-spline definitions
//! there (de Boor evaluation, knot multiplicities, rational weights).
//!
//! Surfaces: IfcPlane, IfcCylindricalSurface, IfcConicalSurface, IfcSphericalSurface, IfcToroidalSurface,
//! IfcSurfaceOfLinearExtrusion, IfcSurfaceOfRevolution, IfcBSplineSurfaceWithKnots, IfcRationalBSplineSurfaceWithKnots,
//! IfcRectangularTrimmedSurface (its basis surface), IfcCurveBoundedPlane (its plane).
//!
//! A face is tessellated in its surface's parameter space: the boundary loops are mapped to (u, v) (periodic directions
//! unwrapped along each loop), interior points are added on a grid, the points are triangulated (Bowyer–Watson
//! Delaunay in a metric-scaled (u, v)), boundary edges recovered, triangles outside the loops dropped, and the inside is
//! evaluated on the surface; boundary points keep their exact 3D positions so neighbouring faces meet.

use std::collections::{HashMap, VecDeque};

use super::ctx::{GR, GeoCtx, GeoErr, bool_, list_or_empty, numd, refs, type_error_read};
use super::curve::curve_points;
use super::math::{M4, V3, add, apply, apply_v, cross, dot, g2, g3, len, norm, rotation, s2, s3, scale, sub};
use super::mesh::Mesh;
use super::profile::profile_shapes;
use crate::jsmath::{JsMath, hypot2, max, min, round, sign};
use crate::model::Id;
use crate::util::usize_f64;

const TAU: f64 = 2.0 * std::f64::consts::PI;

pub fn advanced_brep(g: &mut GeoCtx<'_>, id: Option<Id>, m: &mut Mesh) -> GR<()> {
    let mut shells = vec![g.ra(id, "Outer")?];
    shells.extend(refs(g.a(id, "Voids")?).into_iter().map(Some));
    for sh in shells {
        for f in refs(g.a(sh, "CfsFaces")?) {
            if !surface_face(g, Some(f), m)? {
                planar_face(g, Some(f), m)?;
            }
        }
    }
    Ok(())
}

// ---- loops ----

fn vertex_point(g: &GeoCtx<'_>, v: Option<Id>) -> GR<V3> {
    g.point(g.ra(v, "VertexGeometry")?)
}

/// The loop's edges as point runs, chained; `ring[k]` is true for an edge that closes on itself (a full circle).
#[derive(Clone, Debug, Default)]
pub struct Runs {
    pub runs: Vec<Vec<V3>>,
    pub ring: Vec<bool>,
}

/// IfcEdgeLoop → points (edge curves tessellated, edges chained by their ends).
pub fn edge_loop_points(g: &mut GeoCtx<'_>, lp: Option<Id>) -> GR<Vec<V3>> {
    Ok(flatten(&edge_loop_runs(g, lp)?))
}

pub fn edge_loop_runs(g: &mut GeoCtx<'_>, lp: Option<Id>) -> GR<Runs> {
    let mut runs: Vec<Vec<V3>> = Vec::new();
    let mut ring = Vec::new();
    for oe in refs(g.a(lp, "EdgeList")?) {
        let mut e = Some(oe);
        let mut forward = true;
        if g.t(Some(oe)) == Some("IFCORIENTEDEDGE") {
            e = g.ra(Some(oe), "EdgeElement")?;
            forward = bool_(g.a(Some(oe), "Orientation")?, Some(true)).unwrap_or(true);
        }
        let s = vertex_point(g, g.ra(e, "EdgeStart")?)?;
        let en = vertex_point(g, g.ra(e, "EdgeEnd")?)?;
        let mut pts = vec![s, en];
        if g.t(e) == Some("IFCEDGECURVE") {
            let c = g.ra(e, "EdgeGeometry")?;
            let ct = g.tt(c);
            if let Some(ct) = ct
                && ct != "IFCLINE"
            {
                let mut cp = curve_points(g, c, 0)?;
                if bool_(g.a(e, "SameSense")?, Some(true)) == Some(false) {
                    cp.reverse();
                }
                if cp.len() >= 2 {
                    if ct == "IFCCIRCLE" || ct == "IFCELLIPSE" {
                        cp = arc_between(&cp, &s, &en);
                    } else if let (Some(c0), Some(cl)) = (cp.first(), cp.last())
                        && len(&sub(c0, &s)) > len(&sub(cl, &s))
                    {
                        cp.reverse();
                    }
                    let mut np = vec![s];
                    if cp.len() > 2 {
                        np.extend_from_slice(cp.get(1..cp.len().saturating_sub(1)).unwrap_or(&[]));
                    }
                    np.push(en);
                    pts = np;
                }
            }
        }
        if !forward {
            pts.reverse();
        }
        runs.push(pts);
        ring.push(len(&sub(&s, &en)) < 1e-9);
    }
    // chain: each run continues from the previous one's end
    if runs.len() > 1 {
        let d = |p: &V3, q: &V3| len(&sub(p, q));
        let (first, second) = (runs.first().cloned().unwrap_or_default(), runs.get(1).cloned().unwrap_or_default());
        let (s0, sl) = (second.first().copied().unwrap_or([f64::NAN; 3]), second.last().copied().unwrap_or([f64::NAN; 3]));
        let touch = |p: &V3| min(d(p, &s0), d(p, &sl));
        if let (Some(f0), Some(fl)) = (first.first(), first.last())
            && touch(f0) < touch(fl)
            && let Some(r0) = runs.first_mut()
        {
            r0.reverse();
        }
        for k in 1..runs.len() {
            let last = runs.get(k.saturating_sub(1)).and_then(|r| r.last()).copied().unwrap_or([f64::NAN; 3]);
            if let Some(r) = runs.get_mut(k)
                && let (Some(r0), Some(rl)) = (r.first(), r.last())
                && d(&last, rl) < d(&last, r0)
            {
                r.reverse();
            }
        }
    }
    Ok(Runs { runs, ring })
}

fn flatten(r: &Runs) -> Vec<V3> {
    let mut out: Vec<V3> = Vec::new();
    for pts in &r.runs {
        for p in pts {
            if out.last().is_none_or(|l| len(&sub(l, p)) > 1e-12) {
                out.push(*p);
            }
        }
    }
    if out.len() > 1
        && let (Some(a), Some(b)) = (out.first(), out.last())
        && len(&sub(a, b)) < 1e-12
    {
        out.pop();
    }
    out
}

/// The run of a closed sample loop from the sample nearest a to the one nearest b (going forward).
fn arc_between(cp: &[V3], a: &V3, b: &V3) -> Vec<V3> {
    let closed = match (cp.first(), cp.last()) {
        (Some(f), Some(l)) => len(&sub(f, l)) < 1e-9,
        _ => false,
    };
    let ring: &[V3] = if closed { cp.get(..cp.len().saturating_sub(1)).unwrap_or(&[]) } else { cp };
    let n = ring.len();
    if n == 0 {
        return Vec::new();
    }
    let near = |q: &V3| {
        let mut k = 0usize;
        let mut d = f64::INFINITY;
        for (i, p) in ring.iter().enumerate() {
            let x = len(&sub(p, q));
            if x < d {
                d = x;
                k = i;
            }
        }
        k
    };
    let i = near(a);
    let j = near(b);
    let mut out = Vec::new();
    if i == j {
        for k in 0..=n {
            if let Some(p) = ring.get(i.saturating_add(k).checked_rem(n).unwrap_or(0)) {
                out.push(*p);
            }
        }
        return out;
    }
    let mut k = i;
    loop {
        if let Some(p) = ring.get(k) {
            out.push(*p);
        }
        if k == j || out.len() > n {
            break;
        }
        k = super::math::next(k, n);
    }
    out
}

#[derive(Clone, Debug)]
struct Loop {
    pts: Vec<V3>,
    runs: Option<Runs>,
    rev: bool,
}

fn face_loops(g: &mut GeoCtx<'_>, f: Option<Id>) -> GR<(Option<Loop>, Vec<Loop>)> {
    let mut outer: Option<Loop> = None;
    let mut holes = Vec::new();
    for b in refs(g.a(f, "Bounds")?) {
        let lp = g.ra(Some(b), "Bound")?;
        let lt = g.t(lp);
        let runs = if lt == Some("IFCEDGELOOP") { Some(edge_loop_runs(g, lp)?) } else { None };
        let mut pts = if lt == Some("IFCPOLYLOOP") {
            let mut v = Vec::new();
            for p in refs(g.a(lp, "Polygon")?) {
                v.push(g.point(Some(p))?);
            }
            v
        } else if let Some(r) = &runs {
            flatten(r)
        } else {
            Vec::new()
        };
        if pts.len() < 2 {
            continue;
        }
        let rev = bool_(g.a(Some(b), "Orientation")?, Some(true)) == Some(false);
        if rev {
            pts.reverse();
        }
        let l = Loop { pts, runs, rev };
        if g.t(Some(b)) == Some("IFCFACEOUTERBOUND") && outer.is_none() {
            outer = Some(l);
        } else {
            holes.push(l);
        }
    }
    Ok((outer, holes))
}

/// A face drawn as a flat polygon from its loops (planes, and the fallback for surfaces it does not know).
fn planar_face(g: &mut GeoCtx<'_>, f: Option<Id>, m: &mut Mesh) -> GR<()> {
    let (outer, holes) = face_loops(g, f)?;
    let mut holes: Vec<Vec<V3>> = holes.into_iter().map(|l| l.pts).filter(|h| h.len() >= 3).collect();
    let mut outer = match outer {
        Some(o) if o.pts.len() >= 3 => o.pts,
        _ => {
            if holes.is_empty() {
                return Ok(());
            }
            holes.remove(0)
        }
    };
    if bool_(g.a(f, "SameSense")?, Some(true)) == Some(false) {
        outer.reverse();
        for h in &mut holes {
            h.reverse();
        }
    }
    let mut rings = vec![outer];
    rings.extend(holes);
    m.polygon(&rings);
    Ok(())
}

// ---- surfaces ----

#[derive(Clone, Copy, Debug)]
struct Fr {
    o: V3,
    x: V3,
    y: V3,
    z: V3,
}

fn frame_of(m: &M4) -> Fr {
    Fr { o: [m[12], m[13], m[14]], x: [m[0], m[1], m[2]], y: [m[4], m[5], m[6]], z: [m[8], m[9], m[10]] }
}

/// A swept curve (an open profile's curve) sampled and parameterised by length.
#[derive(Clone, Debug)]
struct SweptCurve {
    pts: Vec<V3>,
    s: Vec<f64>,
    total: f64,
    mid: f64,
    /// the curvature estimate geo-nurbs.js gives the surface (kept for the density code that reads `curv`)
    #[allow(dead_code)]
    curv: f64,
}

impl SweptCurve {
    fn at(&self, x: f64, m: &M4) -> V3 {
        let v = max(0.0, min(self.total, x));
        let mut i = 1usize;
        while i < self.s.len().saturating_sub(1) && self.s.get(i).is_some_and(|&s| s < v) {
            i = i.saturating_add(1);
        }
        let si = |k: usize| self.s.get(k).copied().unwrap_or(f64::NAN);
        let pi = |k: usize| self.pts.get(k).copied().unwrap_or([f64::NAN; 3]);
        let im = i.saturating_sub(1);
        let f = (v - si(im)) / super::math::or(si(i) - si(im), 1.0);
        let p = add(&pi(im), &scale(&sub(&pi(i), &pi(im)), f));
        apply(m, p[0], p[1], p[2])
    }
    fn near(&self, q: &V3, m: &M4) -> f64 {
        let mut best = 0.0;
        let mut bd = f64::INFINITY;
        for (i, w) in self.pts.windows(2).enumerate() {
            let [wa, wb] = w else { continue };
            let a = apply_v(m, wa);
            let b = apply_v(m, wb);
            let ab = sub(&b, &a);
            let l2 = super::math::or(dot(&ab, &ab), 1e-300);
            let f = max(0.0, min(1.0, dot(&sub(q, &a), &ab) / l2));
            let d = len(&sub(q, &add(&a, &scale(&ab, f))));
            if d < bd {
                bd = d;
                let (s0, s1) = (self.s.get(i).copied().unwrap_or(f64::NAN), self.s.get(i.saturating_add(1)).copied().unwrap_or(f64::NAN));
                best = s0 + f * (s1 - s0);
            }
        }
        best
    }
}

fn swept_curve(g: &mut GeoCtx<'_>, prof: Option<Id>) -> GR<Option<SweptCurve>> {
    let t = g.tt(prof);
    let mut pts: Option<Vec<V3>> = None;
    if t == Some("IFCARBITRARYOPENPROFILEDEF") || t == Some("IFCARBITRARYCLOSEDPROFILEDEF") {
        let c = g.ra(prof, if t == Some("IFCARBITRARYOPENPROFILEDEF") { "Curve" } else { "OuterCurve" })?;
        pts = Some(curve_points(g, c, 0)?);
    } else if let Some(tn) = t {
        if g.s.is_a(tn, "IFCCURVE") {
            pts = Some(curve_points(g, prof, 0)?);
        } else if g.s.is_a(tn, "IFCPROFILEDEF") {
            let sh = profile_shapes(g, prof, 0)?;
            if let Some(s0) = sh.first() {
                pts = Some(s0.outer.iter().map(|p| [p[0], p[1], 0.0]).collect());
            }
        }
    }
    let Some(pts) = pts.filter(|p| p.len() >= 2) else { return Ok(None) };
    let mut s = vec![0.0];
    for w in pts.windows(2) {
        if let [a, b] = w {
            let prev = s.last().copied().unwrap_or(0.0);
            s.push(prev + len(&sub(b, a)));
        }
    }
    let total = super::math::or(s.last().copied().unwrap_or(0.0), 1.0);
    let curv = if pts.len() > 2 { 4.0 / total } else { 0.0 };
    Ok(Some(SweptCurve { pts, s, total, mid: total / 2.0, curv }))
}

#[derive(Clone, Debug)]
struct BSpl {
    pu: f64,
    pv: f64,
    nu: usize,
    nv: usize,
    u: Vec<f64>,
    v: Vec<f64>,
    h: Vec<Vec<[f64; 4]>>,
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
    closed_u: bool,
    closed_v: bool,
    grid: Vec<(f64, f64, V3)>,
    du: f64,
    dv: f64,
}

#[derive(Clone, Debug)]
enum Surf {
    Plane,
    Cyl { f: Fr, r: f64, k: f64 },
    Sphere { f: Fr, r: f64 },
    Torus { f: Fr, big: f64, r: f64 },
    Extrusion { m: M4, d: V3, curve: SweptCurve },
    Revolution { m: M4, ao: V3, ad: V3, curve: SweptCurve, rf: V3, rf2: V3 },
    BSpline(Box<BSpl>),
}

/// A surface: at(u, v), inv(p, hint), periods, metric scale.
#[derive(Clone, Debug)]
pub struct Surface {
    s: Surf,
    period: [f64; 2],
    scale_uv: Option<[f64; 2]>,
    domain: Option<[f64; 4]>,
}

impl Surface {
    fn is_plane(&self) -> bool {
        matches!(self.s, Surf::Plane)
    }

    fn at(&self, u: f64, v: f64) -> V3 {
        match &self.s {
            Surf::Plane => [f64::NAN; 3],
            Surf::Cyl { f, r, k } => {
                let rr = r + v * k;
                add(&f.o, &add(&add(&scale(&f.x, rr * u.jcos()), &scale(&f.y, rr * u.jsin())), &scale(&f.z, v)))
            }
            Surf::Sphere { f, r } => add(
                &f.o,
                &add(&add(&scale(&f.x, r * v.jcos() * u.jcos()), &scale(&f.y, r * v.jcos() * u.jsin())), &scale(&f.z, r * v.jsin())),
            ),
            Surf::Torus { f, big, r } => {
                let w = big + r * v.jcos();
                add(&f.o, &add(&add(&scale(&f.x, w * u.jcos()), &scale(&f.y, w * u.jsin())), &scale(&f.z, r * v.jsin())))
            }
            Surf::Extrusion { m, d, curve } => add(&curve.at(u, m), &scale(d, v)),
            Surf::Revolution { m, ao, ad, curve, .. } => {
                let p = curve.at(v, m);
                let r = rotation(ad, u);
                add(ao, &apply_v(&r, &sub(&p, ao)))
            }
            Surf::BSpline(b) => b.at(u, v),
        }
    }

    fn inv(&self, p: &V3, hint: Option<[f64; 2]>) -> [f64; 2] {
        match &self.s {
            Surf::Plane => [f64::NAN; 2],
            Surf::Cyl { f, .. } => {
                let d = sub(p, &f.o);
                [dot(&d, &f.y).jatan2(dot(&d, &f.x)), dot(&d, &f.z)]
            }
            Surf::Sphere { f, r } => {
                let d = sub(p, &f.o);
                [dot(&d, &f.y).jatan2(dot(&d, &f.x)), max(-1.0, min(1.0, dot(&d, &f.z) / super::math::or(*r, 1.0))).jasin()]
            }
            Surf::Torus { f, big, .. } => {
                let d = sub(p, &f.o);
                let u = dot(&d, &f.y).jatan2(dot(&d, &f.x));
                let radial = hypot2(dot(&d, &f.x), dot(&d, &f.y)) - big;
                [u, dot(&d, &f.z).jatan2(radial)]
            }
            Surf::Extrusion { m, d, curve } => {
                let v = dot(&sub(p, &[m[12], m[13], m[14]]), d);
                [curve.near(&sub(p, &scale(d, v)), m), v]
            }
            Surf::Revolution { m, ao, ad, curve, rf, rf2 } => {
                let d = sub(p, ao);
                let h = dot(&d, ad);
                let w = sub(&d, &scale(ad, h));
                let u = dot(&w, rf2).jatan2(dot(&w, rf));
                let back = add(ao, &add(&scale(ad, h), &scale(rf, len(&w))));
                [u, curve.near(&back, m)]
            }
            Surf::BSpline(b) => b.inv(p, hint),
        }
    }
}

impl BSpl {
    fn wrap_u(&self, u: f64) -> f64 {
        if self.closed_u {
            let w = self.u1 - self.u0;
            self.u0 + (((u - self.u0) % w) + w) % w
        } else {
            max(self.u0, min(self.u1, u))
        }
    }
    fn wrap_v(&self, v: f64) -> f64 {
        if self.closed_v {
            let w = self.v1 - self.v0;
            self.v0 + (((v - self.v0) % w) + w) % w
        } else {
            max(self.v0, min(self.v1, v))
        }
    }

    fn at(&self, u: f64, v: f64) -> V3 {
        let u = self.wrap_u(u);
        let v = self.wrap_v(v);
        // a negative degree sums nothing (0 / 0)
        if self.pu < 0.0 || self.pv < 0.0 {
            return [f64::NAN; 3];
        }
        let pu = crate::util::f64_index(self.pu, usize::MAX).unwrap_or(0);
        let pv = crate::util::f64_index(self.pv, usize::MAX).unwrap_or(0);
        let ku = span(&self.u, pu, self.nu, u);
        let kv = span(&self.v, pv, self.nv, v);
        let nu = basis(&self.u, pu, ku, u);
        let nv = basis(&self.v, pv, kv, v);
        let (mut x, mut y, mut z, mut w) = (0.0, 0.0, 0.0, 0.0);
        for (i, bu) in nu.iter().enumerate() {
            let row = self.h.get(ku.saturating_sub(pu).saturating_add(i));
            for (j, bv) in nv.iter().enumerate() {
                let h = row.and_then(|r| r.get(kv.saturating_sub(pv).saturating_add(j))).copied().unwrap_or([f64::NAN; 4]);
                let b = bu * bv;
                x += b * h[0];
                y += b * h[1];
                z += b * h[2];
                w += b * h[3];
            }
        }
        [x / w, y / w, z / w]
    }

    /// Nearest grid sample (or the hint), then Newton steps on the distance (numerical derivatives).
    fn inv(&self, p: &V3, hint: Option<[f64; 2]>) -> [f64; 2] {
        let (mut u, mut v) = (f64::NAN, f64::NAN);
        if let Some([hu, hv]) = hint {
            u = hu;
            v = hv;
        } else {
            let mut bd = f64::INFINITY;
            for (gu, gv, q) in &self.grid {
                let d = len(&sub(q, p));
                if d < bd {
                    bd = d;
                    u = *gu;
                    v = *gv;
                }
            }
        }
        let (du, dv) = (self.du, self.dv);
        for _ in 0..12 {
            // one-sided differences that stay inside the domain (a step past its edge would be clamped to a zero derivative)
            let s = self.at(u, v);
            let hu = if self.closed_u || u + du <= self.u1 { du } else { -du };
            let hv = if self.closed_v || v + dv <= self.v1 { dv } else { -dv };
            let su = scale(&sub(&self.at(u + hu, v), &s), 1.0 / hu);
            let sv = scale(&sub(&self.at(u, v + hv), &s), 1.0 / hv);
            let r = sub(p, &s);
            let (a, b, c, e, f) = (dot(&su, &su), dot(&su, &sv), dot(&sv, &sv), dot(&r, &su), dot(&r, &sv));
            let det = a * c - b * b;
            if det.abs() < 1e-300 {
                break;
            }
            let xu = (e * c - f * b) / det;
            let xv = (a * f - b * e) / det;
            u = if self.closed_u { u + xu } else { max(self.u0, min(self.u1, u + xu)) };
            v = if self.closed_v { v + xv } else { max(self.v0, min(self.v1, v + xv)) };
            if xu.abs() < du && xv.abs() < dv {
                break;
            }
        }
        if self.closed_u {
            u = self.wrap_u(u);
        }
        if self.closed_v {
            v = self.wrap_v(v);
        }
        if hint.is_some() {
            // a hint can trap Newton on the far side of a fold: keep the better of it and a fresh grid start
            let d1 = len(&sub(&self.at(u, v), p));
            if d1 > 1e-6 * (1.0 + len(p)) {
                let fresh = self.inv(p, None);
                if len(&sub(&self.at(fresh[0], fresh[1]), p)) < d1 {
                    return fresh;
                }
            }
        }
        [u, v]
    }
}

fn span(k: &[f64], p: usize, n: usize, x: f64) -> usize {
    let mut s = p;
    while s < n.saturating_sub(1) && k.get(s.saturating_add(1)).is_some_and(|&kk| x >= kk) {
        s = s.saturating_add(1);
    }
    s
}

/// The p + 1 non-zero B-spline basis functions at x in span k (Cox–de Boor).
fn basis(kn: &[f64], p: usize, k: usize, x: f64) -> Vec<f64> {
    let kk = |i: Option<usize>| i.and_then(|i| kn.get(i)).copied().unwrap_or(f64::NAN);
    let mut n = vec![0.0; p.saturating_add(1)];
    if let Some(n0) = n.first_mut() {
        *n0 = 1.0;
    }
    let mut left = vec![0.0; p.saturating_add(1)];
    let mut right = vec![0.0; p.saturating_add(1)];
    for j in 1..=p {
        if let Some(l) = left.get_mut(j) {
            *l = x - kk(k.saturating_add(1).checked_sub(j));
        }
        if let Some(r) = right.get_mut(j) {
            *r = kk(k.checked_add(j)) - x;
        }
        let mut saved = 0.0;
        for r in 0..j {
            let rr = right.get(r.saturating_add(1)).copied().unwrap_or(f64::NAN);
            let ll = left.get(j.saturating_sub(r)).copied().unwrap_or(f64::NAN);
            let den = rr + ll;
            let nr = n.get(r).copied().unwrap_or(f64::NAN);
            let tmp = if super::math::falsy(den) { 0.0 } else { nr / den };
            if let Some(x) = n.get_mut(r) {
                *x = saved + rr * tmp;
            }
            saved = ll * tmp;
        }
        if let Some(x) = n.get_mut(j) {
            *x = saved;
        }
    }
    n
}

/// The highest surface degree evaluated (the evaluation is quadratic in it).
const MAX_DEGREE: f64 = 100.0;
/// The most knots a direction may expand to.
const MAX_KNOTS: f64 = 1_000_000.0;

fn bspline_surface(g: &mut GeoCtx<'_>, sid: Option<Id>, t: &str) -> GR<Option<Surface>> {
    let pu = g.na(sid, "UDegree", 1.0)?;
    let pv = g.na(sid, "VDegree", 1.0)?;
    let rows = list_or_empty(g.a(sid, "ControlPointsList")?)?;
    let mut ctrl: Vec<Vec<V3>> = Vec::with_capacity(rows.len());
    for r in rows {
        let mut row = Vec::new();
        for c in refs(Some(r)) {
            row.push(g.point(Some(c))?);
        }
        ctrl.push(row);
    }
    let nu = ctrl.len();
    let nv = ctrl.first().map_or(0, Vec::len);
    if nu < 2 || nv < 2 {
        return Ok(None);
    }
    let knots = |g: &GeoCtx<'_>, ks: &str, ms: &str| -> GR<Vec<f64>> {
        let ks = list_or_empty(g.a(sid, ks)?)?;
        let ms = g.a(sid, ms)?;
        let mut out = Vec::new();
        for (i, k) in ks.iter().enumerate() {
            let mi = match ms {
                Some(crate::value::Value::List(l)) => l.get(i),
                _ => None,
            };
            let m = super::math::or(numd(mi, 1.0), 1.0);
            let reps = if m > 0.0 { m.ceil() } else { 0.0 };
            if usize_f64(out.len()) + reps > MAX_KNOTS {
                return Err(GeoErr("Invalid array length".into()));
            }
            for _ in 0..super::curve::count(reps)? {
                out.push(numd(Some(k), 0.0));
            }
        }
        Ok(out)
    };
    let u = knots(g, "UKnots", "UMultiplicities")?;
    let v = knots(g, "VKnots", "VMultiplicities")?;
    if usize_f64(u.len()) != usize_f64(nu) + pu + 1.0 || usize_f64(v.len()) != usize_f64(nv) + pv + 1.0 {
        g.note("A B-spline surface with inconsistent knots is drawn flat");
        return Ok(None);
    }
    // degrees past the control net, rows shorter than the first, read H past its end (TypeErrors in the JS)
    if pu > MAX_DEGREE || pv > MAX_DEGREE || pu >= usize_f64(nu) || pv >= usize_f64(nv) || ctrl.iter().any(|r| r.len() < nv) {
        return Err(type_error_read("undefined", "0"));
    }
    let w: Option<Vec<Vec<f64>>> = if t == "IFCRATIONALBSPLINESURFACEWITHKNOTS" {
        let mut out = Vec::new();
        for r in list_or_empty(g.a(sid, "WeightsData")?)? {
            out.push(list_or_empty(Some(r))?.iter().map(|x| numd(Some(x), 1.0)).collect());
        }
        Some(out)
    } else {
        None
    };
    let h: Vec<Vec<[f64; 4]>> = ctrl
        .iter()
        .enumerate()
        .map(|(i, r)| {
            r.iter()
                .enumerate()
                .map(|(j, c)| {
                    let wt = w.as_ref().map_or(1.0, |w| w.get(i).and_then(|row| row.get(j)).copied().unwrap_or(1.0));
                    [c[0] * wt, c[1] * wt, c[2] * wt, wt]
                })
                .collect()
        })
        .collect();
    let idx = |k: &[f64], i: f64| crate::util::f64_index(i, k.len()).and_then(|i| k.get(i)).copied().unwrap_or(f64::NAN);
    let (u0, u1, v0, v1) = (idx(&u, pu), u.get(nu).copied().unwrap_or(f64::NAN), idx(&v, pv), v.get(nv).copied().unwrap_or(f64::NAN));
    let closed_u = bool_(g.a(sid, "UClosed")?, Some(false)) == Some(true);
    let closed_v = bool_(g.a(sid, "VClosed")?, Some(false)) == Some(true);
    let mut b = BSpl {
        pu,
        pv,
        nu,
        nv,
        u,
        v,
        h,
        u0,
        u1,
        v0,
        v1,
        closed_u,
        closed_v,
        grid: Vec::new(),
        du: super::math::or((u1 - u0) * 1e-6, 1e-9),
        dv: super::math::or((v1 - v0) * 1e-6, 1e-9),
    };
    // inverse: nearest grid sample, then Newton steps
    const G: usize = 24;
    let gf = usize_f64(G);
    let mut grid = Vec::with_capacity((G + 1) * (G + 1));
    for i in 0..=G {
        for j in 0..=G {
            let uu = u0 + (u1 - u0) * usize_f64(i) / gf;
            let vv = v0 + (v1 - v0) * usize_f64(j) / gf;
            grid.push((uu, vv, b.at(uu, vv)));
        }
    }
    b.grid = grid;
    let period = [if closed_u { u1 - u0 } else { 0.0 }, if closed_v { v1 - v0 } else { 0.0 }];
    Ok(Some(Surface { s: Surf::BSpline(Box::new(b)), period, scale_uv: None, domain: Some([u0, u1, v0, v1]) }))
}

pub fn surface_of(g: &mut GeoCtx<'_>, sid: Option<Id>, depth: u32) -> GR<Option<Surface>> {
    let Some(t) = g.tt(sid) else { return Ok(None) };
    if depth > 4 {
        return Ok(None);
    }
    if t == "IFCRECTANGULARTRIMMEDSURFACE" || t == "IFCCURVEBOUNDEDPLANE" || t == "IFCCURVEBOUNDEDSURFACE" {
        let bs = g.ra(sid, "BasisSurface")?;
        return surface_of(g, bs, depth.saturating_add(1));
    }
    let pos = |g: &mut GeoCtx<'_>| -> GR<M4> {
        let p = g.ra(sid, "Position")?;
        g.axis2(p)
    };
    let n = |g: &GeoCtx<'_>, k: &str| g.na(sid, k, 0.0);
    let s = |s: Surf, period: [f64; 2], scale_uv: Option<[f64; 2]>| Surface { s, period, scale_uv, domain: None };
    match t {
        "IFCPLANE" => {
            let _ = frame_of(&pos(g)?);
            Ok(Some(s(Surf::Plane, [0.0, 0.0], None)))
        }
        "IFCCYLINDRICALSURFACE" | "IFCCONICALSURFACE" => {
            let f = frame_of(&pos(g)?);
            let r = n(g, "Radius")?;
            let k = if t == "IFCCONICALSURFACE" { (n(g, "SemiAngle")? * g.angle_unit).jtan() } else { 0.0 };
            Ok(Some(s(Surf::Cyl { f, r, k }, [TAU, 0.0], Some([r, 1.0]))))
        }
        "IFCSPHERICALSURFACE" => {
            let f = frame_of(&pos(g)?);
            let r = n(g, "Radius")?;
            Ok(Some(s(Surf::Sphere { f, r }, [TAU, 0.0], Some([r, r]))))
        }
        "IFCTOROIDALSURFACE" => {
            let f = frame_of(&pos(g)?);
            let big = n(g, "MajorRadius")?;
            let r = n(g, "MinorRadius")?;
            Ok(Some(s(Surf::Torus { f, big, r }, [TAU, TAU], Some([big, r]))))
        }
        "IFCSURFACEOFLINEAREXTRUSION" => {
            let m = pos(g)?;
            let dir = g.dirv(g.ra(sid, "ExtrudedDirection")?, [0.0, 0.0, 1.0])?;
            let sc = g.ra(sid, "SweptCurve")?;
            let Some(curve) = swept_curve(g, sc)? else { return Ok(None) };
            let a = apply_v(&m, &dir);
            let d = norm(&[a[0] - m[12], a[1] - m[13], a[2] - m[14]]);
            Ok(Some(s(Surf::Extrusion { m, d, curve }, [0.0, 0.0], Some([1.0, 1.0]))))
        }
        "IFCSURFACEOFREVOLUTION" => {
            let m = pos(g)?;
            let ax = g.ra(sid, "AxisPosition")?;
            let ao = apply_v(&m, &g.point(g.ra(ax, "Location")?)?);
            let ad = norm(&sub(&apply_v(&m, &g.dirv(g.ra(ax, "Axis")?, [0.0, 0.0, 1.0])?), &[m[12], m[13], m[14]]));
            let sc = g.ra(sid, "SweptCurve")?;
            let Some(curve) = swept_curve(g, sc)? else { return Ok(None) };
            // the angle about the axis measured from the generating curve's own half-plane
            let rf = {
                let p = curve.at(curve.mid, &m);
                let d = sub(&p, &ao);
                let w = sub(&d, &scale(&ad, dot(&d, &ad)));
                if len(&w) > 1e-12 { norm(&w) } else { norm(&cross(&ad, &[1.0, 0.0, 0.0])) }
            };
            let rf2 = cross(&ad, &rf);
            Ok(Some(s(Surf::Revolution { m, ao, ad, curve, rf, rf2 }, [TAU, 0.0], Some([1.0, 1.0]))))
        }
        "IFCBSPLINESURFACEWITHKNOTS" | "IFCRATIONALBSPLINESURFACEWITHKNOTS" => bspline_surface(g, sid, t),
        _ => {
            let p = g.pretty(t);
            g.note(format!("{p} surfaces are not drawn yet; their faces are drawn flat"));
            Ok(None)
        }
    }
}

// ---- faces ----

/// A (u, v) point; `free` = the directions where the surface collapses and the parameter means nothing (bit 0 = u).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Uv {
    q: [f64; 2],
    free: u8,
}

impl Uv {
    fn plain(q: [f64; 2]) -> Uv {
        Uv { q, free: 0 }
    }
}

#[derive(Clone, Debug)]
struct UvLoop {
    p: Vec<V3>,
    uv: Vec<Uv>,
    st: Vec<[f64; 2]>,
    outer: bool,
}

/// Tessellate one face on its surface into m. false when the face is not on a curved surface it can read.
pub fn surface_face(g: &mut GeoCtx<'_>, f: Option<Id>, m: &mut Mesh) -> GR<bool> {
    let ft = g.t(f);
    if ft != Some("IFCADVANCEDFACE") && ft != Some("IFCFACESURFACE") {
        return Ok(false);
    }
    let sid = g.ra(f, "FaceSurface")?;
    let st = g.tt(sid);
    if st.is_none() || st == Some("IFCPLANE") {
        return Ok(false);
    }
    let Some(s) = surface_of(g, sid, 0)? else { return Ok(false) };
    if s.is_plane() {
        return Ok(false);
    }
    let same = bool_(g.a(f, "SameSense")?, Some(true)) != Some(false);
    let (outer, holes) = face_loops(g, f)?;
    let mut loops: Vec<(Loop, bool)> = Vec::new();
    if let Some(o) = outer {
        loops.push((o, true));
    }
    loops.extend(holes.into_iter().map(|h| (h, false)));
    loops.retain(|(l, _)| l.pts.len() >= 2);
    if loops.is_empty() {
        return Ok(true);
    }
    // 1. loops → (u, v), unwrapping periodic directions along each loop; full-circle edges run whichever way closes the loop
    let mut uv_loops = Vec::with_capacity(loops.len());
    for (l, o) in &loops {
        let (p, uv) = closing_uv(&s, l)?;
        uv_loops.push(UvLoop { p, uv, st: Vec::new(), outer: *o });
    }
    // 2. a loop that does not close in (u, v) wraps round a periodic direction: two such loops make one band
    let mut closed_l: Vec<UvLoop> = Vec::new();
    let mut wrapping: Vec<UvLoop> = Vec::new();
    for l in uv_loops {
        let (Some(a), Some(b)) = (l.uv.first(), l.uv.last()) else { return Err(type_error_read("undefined", "0")) };
        let gap_u = (b.q[0] - a.q[0]).abs();
        let gap_v = (b.q[1] - a.q[1]).abs();
        let p = s.period;
        if (!super::math::falsy(p[0]) && (gap_u - p[0]).abs() < p[0] * 0.05)
            || (!super::math::falsy(p[1]) && (gap_v - p[1]).abs() < p[1] * 0.05)
        {
            wrapping.push(l);
        } else {
            closed_l.push(l);
        }
    }
    if wrapping.len() == 2 {
        if let (Some(w0), Some(w1)) = (wrapping.first(), wrapping.get(1)) {
            let mut b = band(w0, w1, &s)?;
            b.outer = w0.outer || w1.outer;
            closed_l.push(b);
        }
    } else if !wrapping.is_empty() {
        g.note("A face wrapping round a curved surface is drawn flat");
        return Ok(false);
    }
    if closed_l.is_empty() {
        return Ok(true);
    }
    // boundary points that are not on the face's surface are put back on it; points on it keep their exact place
    {
        let mut lo3 = [f64::INFINITY; 3];
        let mut hi3 = [f64::NEG_INFINITY; 3];
        let mut any = false;
        for l in &closed_l {
            for q in &l.p {
                any = true;
                for k in 0..3 {
                    *s3(&mut lo3, k) = min(g3(&lo3, k), g3(q, k));
                    *s3(&mut hi3, k) = max(g3(&hi3, k), g3(q, k));
                }
            }
        }
        let _ = any;
        let tol = max(1e-9, len(&sub(&hi3, &lo3)) * 1e-3);
        for l in &mut closed_l {
            let uv = &l.uv;
            for (i, q) in l.p.iter_mut().enumerate() {
                let Some(w) = uv.get(i) else { return Err(type_error_read("undefined", "0")) };
                let on = s.at(w.q[0], w.q[1]);
                if len(&sub(&on, q)) > tol {
                    *q = on;
                }
            }
        }
    }
    // 3. metric scale for the parameter directions (u in radians, v in length …)
    let all: Vec<[f64; 2]> = closed_l.iter().flat_map(|l| l.uv.iter().map(|w| w.q)).collect();
    let [mu, mv] = metric(&s, &all);
    for l in &mut closed_l {
        l.st = l.uv.iter().map(|w| [w.q[0] * mu, w.q[1] * mv]).collect();
        // repeated points (where a pole meets a seam, a loop's closing point) would never enter the triangulation
        let mut keep: Vec<usize> = Vec::new();
        for i in 0..l.st.len() {
            let a = l.st.get(i).copied().unwrap_or([f64::NAN; 2]);
            if let Some(&last) = keep.last() {
                let b = l.st.get(last).copied().unwrap_or([f64::NAN; 2]);
                if (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9 {
                    continue;
                }
            }
            keep.push(i);
        }
        while keep.len() > 1 {
            let (Some(&k0), Some(&kl)) = (keep.first(), keep.last()) else { break };
            let a = l.st.get(k0).copied().unwrap_or([f64::NAN; 2]);
            let b = l.st.get(kl).copied().unwrap_or([f64::NAN; 2]);
            if (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9 {
                keep.pop();
            } else {
                break;
            }
        }
        let st: Vec<[f64; 2]> = keep.iter().filter_map(|&i| l.st.get(i).copied()).collect();
        let uv: Vec<Uv> = keep.iter().filter_map(|&i| l.uv.get(i).copied()).collect();
        let p: Vec<V3> = keep.iter().filter_map(|&i| l.p.get(i).copied()).collect();
        l.st = st;
        l.uv = uv;
        l.p = p;
    }
    // the outer loop: the one the file marks (IfcFaceOuterBound), else the largest |area|
    let area = |p: &[[f64; 2]]| {
        let mut s2 = 0.0;
        let n = p.len();
        for (i, a) in p.iter().enumerate() {
            let b = p.get(super::math::next(i, n)).unwrap_or(a);
            s2 += a[0] * b[1] - b[0] * a[1];
        }
        s2 / 2.0
    };
    closed_l.sort_by(|a, b| {
        let d1 = f64::from(u8::from(b.outer)) - f64::from(u8::from(a.outer));
        let d = if d1 != 0.0 { d1 } else { area(&b.st).abs() - area(&a.st).abs() };
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let mut it = closed_l.into_iter();
    let Some(outer_l) = it.next() else { return Ok(true) };
    let mut hole_l: Vec<UvLoop> = it.collect();
    // on a periodic surface a hole may have been unwrapped a whole period away from the outer loop: bring it back
    for d in 0..2usize {
        let per = if d == 0 { s.period[0] } else { s.period[1] };
        if super::math::falsy(per) {
            continue;
        }
        let pp = per * if d == 0 { mu } else { mv };
        let comp = |q: &[f64; 2]| if d == 0 { q[0] } else { q[1] };
        let o_lo = outer_l.st.iter().fold(f64::INFINITY, |a, q| min(a, comp(q)));
        let o_hi = outer_l.st.iter().fold(f64::NEG_INFINITY, |a, q| max(a, comp(q)));
        for l in &mut hole_l {
            let c = l.st.iter().fold(0.0, |s2, q| s2 + comp(q)) / usize_f64(l.st.len());
            let mut k = 0i32;
            while c + f64::from(k) * pp < o_lo && k < 8 {
                k = k.saturating_add(1);
            }
            while c + f64::from(k) * pp > o_hi && k > -8 {
                k = k.saturating_sub(1);
            }
            if k != 0 {
                let kf = f64::from(k);
                for q in &mut l.st {
                    if d == 0 {
                        q[0] += kf * pp;
                    } else {
                        q[1] += kf * pp;
                    }
                }
                for w in &mut l.uv {
                    if d == 0 {
                        w.q[0] += kf * s.period[0];
                    } else {
                        w.q[1] += kf * s.period[1];
                    }
                }
            }
        }
    }
    // 4. density: the surface's normal is sampled over the loops' box; the spacing turns the normal by about 0.2 rad
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in &outer_l.st {
        lo = [min(lo[0], p[0]), min(lo[1], p[1])];
        hi = [max(hi[0], p[0]), max(hi[1], p[1])];
    }
    let ext = super::math::or(max(hi[0] - lo[0], hi[1] - lo[1]), 1.0);
    let nrm_at = |x: f64, y: f64| -> Option<V3> {
        let (u, v) = (x / mu, y / mv);
        let ex = (if x < (lo[0] + hi[0]) / 2.0 { 1.0 } else { -1.0 }) * 1e-4 * ext;
        let ey = (if y < (lo[1] + hi[1]) / 2.0 { 1.0 } else { -1.0 }) * 1e-4 * ext;
        let p0 = s.at(u, v);
        let n = cross(&scale(&sub(&s.at((x + ex) / mu, v), &p0), sign(ex)), &scale(&sub(&s.at(u, (y + ey) / mv), &p0), sign(ey)));
        if len(&n) > 0.0 { Some(norm(&n)) } else { None }
    };
    const K: usize = 8;
    let kf = usize_f64(K);
    let mut turn = 0.0;
    for i in 0..=K {
        for j in 0..=K {
            let x = lo[0] + (hi[0] - lo[0]) * usize_f64(i) / kf;
            let y = lo[1] + (hi[1] - lo[1]) * usize_f64(j) / kf;
            let Some(n0) = nrm_at(x, y) else { continue };
            let ang = |n1: Option<V3>| n1.map_or(0.0, |n1| max(-1.0, min(1.0, dot(&n0, &n1))).jacos());
            if i < K {
                let n1 = nrm_at(lo[0] + (hi[0] - lo[0]) * usize_f64(i.saturating_add(1)) / kf, y);
                turn = max(turn, ang(n1) / super::math::or((hi[0] - lo[0]) / kf, 1.0));
            }
            if j < K {
                let n1 = nrm_at(x, lo[1] + (hi[1] - lo[1]) * usize_f64(j.saturating_add(1)) / kf);
                turn = max(turn, ang(n1) / super::math::or((hi[1] - lo[1]) / kf, 1.0));
            }
        }
    }
    let h = max(ext / 96.0, min(ext / 4.0, if turn > 0.0 { 0.2 / turn } else { ext / 4.0 }));
    // 5. points: the loops' own points only (a neighbouring face shares exactly these), interior grid points away from them
    let mut pts: Vec<[f64; 2]> = Vec::new();
    let mut xyz: Vec<V3> = Vec::new();
    let mut rings: Vec<Vec<usize>> = Vec::new();
    for l in std::iter::once(&outer_l).chain(hole_l.iter()) {
        let mut ids = Vec::with_capacity(l.st.len());
        for (i, q) in l.st.iter().enumerate() {
            ids.push(pts.len());
            pts.push(*q);
            xyz.push(l.p.get(i).copied().unwrap_or([f64::NAN; 3]));
        }
        rings.push(ids);
    }
    let polys: Vec<&Vec<[f64; 2]>> = std::iter::once(&outer_l.st).chain(hole_l.iter().map(|l| &l.st)).collect();
    // inside the outer loop and in none of the holes (inner loops lying outside the outer one take nothing away)
    let inside = |q: &[f64; 2]| polys.first().is_some_and(|o| pip(q, o)) && !polys.iter().skip(1).any(|p| pip(q, p));
    let mut segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for r in &rings {
        let n = r.len();
        for i in 0..n {
            let (Some(&a), Some(&b)) = (r.get(i), r.get(super::math::next(i, n))) else { continue };
            segs.push((pts.get(a).copied().unwrap_or([f64::NAN; 2]), pts.get(b).copied().unwrap_or([f64::NAN; 2])));
        }
    }
    let mut steps = 0u64;
    let mut x = lo[0] + h / 2.0;
    while x < hi[0] {
        let mut y = lo[1] + h / 2.0;
        while y < hi[1] {
            steps = steps.saturating_add(1);
            if steps > 4_000_000 {
                return Err(GeoErr("a face grid that never ends".into()));
            }
            let q = [x, y];
            if inside(&q) && !segs.iter().any(|(a, b)| seg_dist(&q, a, b) < h * 0.45) {
                pts.push(q);
                xyz.push(s.at(x / mu, y / mv));
            }
            y += h;
        }
        x += h;
    }
    // 6. triangulate, then make every boundary segment an edge (constrained Delaunay, Sloan's edge recovery); then keep
    // what is inside the loops
    let mut tris = delaunay(&pts)?;
    let mut segs_idx = Vec::new();
    for r in &rings {
        let n = r.len();
        for i in 0..n {
            if let (Some(&a), Some(&b)) = (r.get(i), r.get(super::math::next(i, n))) {
                segs_idx.push((a, b));
            }
        }
    }
    recover_edges(&pts, &mut tris, &segs_idx);
    let base = m.vertex_count();
    let mut keep: Vec<usize> = Vec::new();
    for t in tris.as_chunks::<3>().0.iter() {
        let &[ia, ib, ic] = t;
        let (a, b, c) = (pts.get(ia), pts.get(ib), pts.get(ic));
        let (Some(a), Some(b), Some(c)) = (a, b, c) else { continue };
        let cen = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
        if inside(&cen) {
            keep.extend_from_slice(&[ia, ib, ic]);
        }
    }
    if keep.is_empty() {
        return Ok(true);
    }
    // 7. orientation: the surface normal (finite differences), reversed when SameSense is false
    let want = |i: usize| -> V3 {
        let q = pts.get(i).copied().unwrap_or([f64::NAN; 2]);
        let (u, v) = (q[0] / mu, q[1] / mv);
        let e = 1e-5 * (1.0 + u.abs());
        let e2 = 1e-5 * (1.0 + v.abs());
        let p0 = s.at(u, v);
        let n = cross(&sub(&s.at(u + e, v), &p0), &sub(&s.at(u, v + e2), &p0));
        if same { n } else { scale(&n, -1.0) }
    };
    let xp = |k: usize| xyz.get(k).copied().unwrap_or([f64::NAN; 3]);
    let mut agree = 0.0;
    for t in keep.as_chunks::<3>().0.iter() {
        let &[ia, ib, ic] = t;
        let n = cross(&sub(&xp(ib), &xp(ia)), &sub(&xp(ic), &xp(ia)));
        agree += sign(dot(&n, &want(ia)));
    }
    let flip = agree < 0.0;
    if !m.room(xyz.len()) {
        return Ok(true);
    }
    for p in &xyz {
        m.pos.extend_from_slice(p);
        m.nrm.extend_from_slice(&[0.0, 0.0, 1.0]);
    }
    let u32_of = |k: usize| u32::try_from(base.saturating_add(k)).unwrap_or(u32::MAX);
    let mut acc = vec![0.0f64; xyz.len().saturating_mul(3)];
    for t in keep.as_chunks::<3>().0.iter() {
        let &[a, b0, c0] = t;
        let (b, c) = if flip { (c0, b0) } else { (b0, c0) };
        m.idx.extend_from_slice(&[u32_of(a), u32_of(b), u32_of(c)]);
        // vertex normals from the triangles around each vertex (smooth shading on curved faces)
        let n = cross(&sub(&xp(b), &xp(a)), &sub(&xp(c), &xp(a)));
        for k in [a, b, c] {
            if let Some(s3) = acc.get_mut(k.saturating_mul(3)..k.saturating_mul(3).saturating_add(3)) {
                for (a, b) in s3.iter_mut().zip(n.iter()) {
                    *a += b;
                }
            }
        }
    }
    for (k, s3) in acc.as_chunks::<3>().0.iter().enumerate() {
        let n = norm(&[s3[0], s3[1], s3[2]]);
        let at = base.saturating_add(k).saturating_mul(3);
        if let Some(dst) = m.nrm.get_mut(at..at.saturating_add(3)) {
            dst.copy_from_slice(&n);
        }
    }
    Ok(true)
}

fn closes(s: &Surface, uv: &[Uv]) -> bool {
    let (Some(a), Some(b)) = (uv.first(), uv.last()) else { return false };
    (0..2).all(|k| {
        let p = g2(&s.period, k);
        super::math::falsy(p) || (g2(&b.q, k) - g2(&a.q, k)).abs() < p * 0.25
    })
}

fn within(s: &Surface, uv: &[Uv]) -> bool {
    (0..2).all(|d| {
        let p = g2(&s.period, d);
        if super::math::falsy(p) {
            return true;
        }
        let hi = uv.iter().fold(f64::NEG_INFINITY, |a, q| max(a, g2(&q.q, d)));
        let lo = uv.iter().fold(f64::INFINITY, |a, q| min(a, g2(&q.q, d)));
        hi - lo <= p * 1.001
    })
}

fn uv_area(p: &[Uv]) -> f64 {
    let mut s = 0.0;
    let n = p.len();
    for (i, a) in p.iter().enumerate() {
        let b = p.get(super::math::next(i, n)).unwrap_or(a);
        s += a.q[0] * b.q[1] - b.q[0] * a.q[1];
    }
    s / 2.0
}

/// (u, v) of a point, with prev as the hint; where the surface collapses that parameter keeps prev's value.
fn inv_at(s: &Surface, p: &V3, prev: Option<&Uv>) -> Uv {
    let mut q = Uv::plain(s.inv(p, prev.map(|w| w.q)));
    let p0 = s.at(q.q[0], q.q[1]);
    for d in 0..2usize {
        // step both ways: at the end of an open domain one step is clamped and moves nothing
        let dom = s.domain.map_or(1.0, |dm| if d == 0 { dm[1] - dm[0] } else { dm[3] - dm[2] });
        let span = super::math::or(super::math::or(g2(&s.period, d), dom), 1.0);
        let e = 1e-3 * span;
        let tiny = 1e-9 * (1.0 + len(&p0));
        let moved = [e, -e].iter().any(|&x| {
            let (du, dv) = if d == 0 { (x, 0.0) } else { (0.0, x) };
            len(&sub(&s.at(q.q[0] + du, q.q[1] + dv), &p0)) > tiny
        });
        if !moved {
            match prev {
                Some(pv) => *s2(&mut q.q, d) = g2(&pv.q, d),
                None => q.free |= 1 << d,
            }
        }
    }
    q
}

/// A loop that starts on a pole: the first points' free parameter takes the first ordinary point's value.
fn settle_start(uv: &mut [Uv]) {
    let Some(f) = uv.first().map(|w| w.free).filter(|&f| f != 0) else { return };
    let j = uv.iter().position(|w| w.free == 0);
    if let Some(j) = j
        && j > 0
    {
        let src = uv.get(j).map_or([f64::NAN; 2], |w| w.q);
        for w in uv.iter_mut().take(j) {
            for d in 0..2 {
                if f & (1 << d) != 0 {
                    *s2(&mut w.q, d) = g2(&src, d);
                }
            }
        }
        for w in uv.iter_mut().take(j) {
            w.free = 0;
        }
    }
}

/// Loop points → (p, uv) with periodic directions unwrapped so the loop is continuous.
fn to_uv(s: &Surface, pts: &[V3]) -> (Vec<V3>, Vec<Uv>) {
    let mut uv: Vec<Uv> = Vec::with_capacity(pts.len());
    let mut prev: Option<Uv> = None;
    for p in pts {
        let mut q = inv_at(s, p, prev.as_ref());
        if let Some(pv) = prev {
            for k in 0..2 {
                let pp = g2(&s.period, k);
                if super::math::falsy(pp) {
                    continue;
                }
                *s2(&mut q.q, k) -= round((g2(&q.q, k) - g2(&pv.q, k)) / pp) * pp;
            }
        }
        uv.push(q);
        prev = if q.free != 0 { None } else { Some(q) };
    }
    settle_start(&mut uv);
    (pts.to_vec(), uv)
}

#[derive(Clone)]
struct Run {
    pts: Vec<V3>,
    ring: bool,
    pole: bool,
}

/// A loop's (u, v) on a periodic surface, made to close: every reading of full-circle edges (either way round) and of
/// poles (shifted by −1, 0, +1 periods) is tried; of those that close, the one enclosing the most of the plane wins.
fn closing_uv(s: &Surface, lp: &Loop) -> GR<(Vec<V3>, Vec<Uv>)> {
    let plain = to_uv(s, &lp.pts);
    let per = s.period;
    let Some(r) = &lp.runs else { return Ok(plain) };
    if (super::math::falsy(per[0]) && super::math::falsy(per[1])) || (closes(s, &plain.1) && within(s, &plain.1)) {
        return Ok(plain);
    }
    let mut rr: Vec<Run> = r
        .runs
        .iter()
        .enumerate()
        .map(|(k, pts)| Run { pts: pts.clone(), ring: r.ring.get(k).copied().unwrap_or(false), pole: false })
        .collect();
    if lp.rev {
        rr.reverse();
        for x in &mut rr {
            x.pts.reverse();
        }
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for x in &rr {
        for p in &x.pts {
            for k in 0..3 {
                *s3(&mut lo, k) = min(g3(&lo, k), g3(p, k));
                *s3(&mut hi, k) = max(g3(&hi, k), g3(p, k));
            }
        }
    }
    let size = max(1e-12, len(&sub(&hi, &lo)));
    for x in &mut rr {
        // a pole: an edge shrunk to a point, or a closed edge whose points all land on one (u, v)
        let mut l = 0.0;
        for w in x.pts.windows(2) {
            if let [a, b] = w {
                l += len(&sub(b, a));
            }
        }
        let mut span = 0.0;
        let mut collapsed = false;
        if x.ring {
            let p0x = x.pts.first().copied().ok_or_else(|| type_error_read("undefined", "0"))?;
            let q0 = s.inv(&p0x, None);
            let p0 = s.at(q0[0], q0[1]);
            for p in &x.pts {
                let q = s.inv(p, None);
                span = max(span, len(&sub(&s.at(q[0], q[1]), &p0)));
            }
            // its vertex sits where the surface collapses: moving along the periodic direction does not move the point
            collapsed = (0..2).any(|d| {
                !super::math::falsy(g2(&per, d))
                    && [0.25, 0.5].iter().all(|&f| {
                        let (du, dv) = if d == 0 { (f * per[0], 0.0) } else { (0.0, f * per[1]) };
                        len(&sub(&s.at(q0[0] + du, q0[1] + dv), &p0)) < size * 1e-6
                    })
            });
        }
        // an edge whose curve strays off the face's surface is drawn like a pole
        let mut off = 0.0;
        let step = (x.pts.len() / 12).max(1);
        for p in x.pts.iter().step_by(step) {
            let q = s.inv(p, None);
            off = max(off, len(&sub(&s.at(q[0], q[1]), p)));
        }
        let junk = off > size * 1e-3;
        x.pole = l < size * 1e-6 || (x.ring && (span < size * 1e-6 || collapsed)) || junk;
    }
    let Some(first) = rr.iter().position(|x| !x.pole) else { return Ok(plain) };
    rr.rotate_left(first);
    let rings: Vec<usize> = rr.iter().enumerate().filter(|(_, x)| x.ring && !x.pole).map(|(i, _)| i).collect();
    let poles: Vec<usize> = rr.iter().enumerate().filter(|(_, x)| x.pole).map(|(i, _)| i).collect();
    if rings.len().saturating_add(poles.len()) > 6 {
        return Ok(plain);
    }
    // each pole run may wrap by -1, 0 or +1 periods in each periodic direction
    let mut opts: Vec<[f64; 2]> = Vec::new();
    let su: &[f64] = if super::math::falsy(per[0]) { &[0.0] } else { &[-1.0, 0.0, 1.0] };
    let sv: &[f64] = if super::math::falsy(per[1]) { &[0.0] } else { &[-1.0, 0.0, 1.0] };
    for a in su {
        for b in sv {
            opts.push([a * per[0], b * per[1]]);
        }
    }
    let no = opts.len();
    let combos = (1usize << rings.len()).saturating_mul(no.saturating_pow(u32::try_from(poles.len()).unwrap_or(0)));
    let mut best: Option<(Vec<V3>, Vec<Uv>)> = None;
    let mut best_area = 1e-12;
    for c in 0..combos {
        let mut rest = c;
        let mut flip = vec![false; rr.len()];
        let mut shift = vec![[0.0, 0.0]; rr.len()];
        for &i in &rings {
            if rest & 1 != 0
                && let Some(f) = flip.get_mut(i)
            {
                *f = true;
            }
            rest >>= 1;
        }
        for &i in &poles {
            if let Some(sh) = shift.get_mut(i) {
                *sh = opts.get(rest.checked_rem(no).unwrap_or(0)).copied().unwrap_or([0.0; 2]);
            }
            rest = rest.checked_div(no).unwrap_or(0);
        }
        let mut uv: Vec<Uv> = Vec::new();
        let mut p: Vec<V3> = Vec::new();
        let mut prev: Option<Uv> = None;
        let mut pending: Option<(usize, usize)> = None; // (run index, its point count)
        for (i, run) in rr.iter().enumerate() {
            let pts: Vec<V3> = if flip.get(i).copied().unwrap_or(false) {
                let mut v = run.pts.clone();
                v.reverse();
                v
            } else {
                run.pts.clone()
            };
            if run.pole {
                pending = Some((i, pts.len()));
                continue;
            }
            for (j, pj) in pts.iter().enumerate() {
                let mut q = inv_at(s, pj, prev.as_ref());
                if let Some(pv) = prev {
                    for d in 0..2 {
                        let pd = g2(&per, d);
                        if !super::math::falsy(pd) {
                            *s2(&mut q.q, d) -= round((g2(&q.q, d) - g2(&pv.q, d)) / pd) * pd;
                        }
                    }
                    if j == 0
                        && let Some((pi, n)) = pending.take()
                    {
                        // the pole: straight along it from where the loop reached it to where it leaves, plus whole periods
                        let sh = shift.get(pi).copied().unwrap_or([0.0; 2]);
                        q.q[0] += sh[0];
                        q.q[1] += sh[1];
                        let a = pv.q;
                        let nf = usize_f64(n) - 1.0;
                        for t in 1..n.saturating_sub(1) {
                            let f = usize_f64(t) / nf;
                            let w = [a[0] + (q.q[0] - a[0]) * f, a[1] + (q.q[1] - a[1]) * f];
                            uv.push(Uv::plain(w));
                            p.push(s.at(w[0], w[1]));
                        }
                    }
                }
                if !uv.is_empty() && j == 0 && p.last().is_some_and(|l| len(&sub(l, pj)) < 1e-12) {
                    prev = Some(q);
                    continue;
                }
                uv.push(q);
                p.push(*pj);
                prev = if q.free != 0 { None } else { Some(q) };
            }
        }
        if let Some((pi, n)) = pending {
            // a pole closes the loop: run along it back to the start, by whole periods
            let a = prev.ok_or_else(|| type_error_read("null", "0"))?.q;
            let u0 = uv.first().ok_or_else(|| type_error_read("undefined", "0"))?.q;
            let mut q = u0;
            let sh = shift.get(pi).copied().unwrap_or([0.0; 2]);
            for d in 0..2 {
                if !super::math::falsy(g2(&per, d)) {
                    *s2(&mut q, d) = g2(&u0, d) + g2(&sh, d) + round((g2(&a, d) - g2(&u0, d)) / g2(&per, d)) * g2(&per, d);
                }
            }
            // points on the pole come from the surface (an exporter's curve there may leave it)
            let nf = usize_f64(n) - 1.0;
            for t in 1..n.saturating_sub(1) {
                let f = usize_f64(t) / nf;
                let w = [a[0] + (q[0] - a[0]) * f, a[1] + (q[1] - a[1]) * f];
                uv.push(Uv::plain(w));
                p.push(s.at(w[0], w[1]));
            }
            uv.push(Uv::plain(q));
            p.push(s.at(q[0], q[1]));
        }
        settle_start(&mut uv);
        // of the readings that close, the one enclosing the most of the parameter plane; a face covers at most one
        // period in each periodic direction
        if within(s, &uv) && closes(s, &uv) {
            let a = uv_area(&uv).abs();
            if a > best_area {
                best_area = a;
                best = Some((p, uv));
            }
        }
    }
    Ok(best.unwrap_or(plain))
}

/// Two loops that each wrap once round a periodic direction → one closed loop.
fn band(a: &UvLoop, b: &UvLoop, s: &Surface) -> GR<UvLoop> {
    let k = if super::math::falsy(s.period[0]) { 1 } else { 0 };
    let pp = g2(&s.period, k);
    let au = &a.uv;
    let (Some(a0), Some(al)) = (au.first(), au.last()) else { return Err(type_error_read("undefined", "0")) };
    let dir_a = super::math::or(sign(g2(&al.q, k) - g2(&a0.q, k)), 1.0);
    let mut bu: Vec<Uv> = b.uv.clone();
    let mut bp: Vec<V3> = b.p.clone();
    let (Some(b0), Some(bl)) = (bu.first(), bu.last()) else { return Err(type_error_read("undefined", "0")) };
    if super::math::or(sign(g2(&bl.q, k) - g2(&b0.q, k)), 1.0) == dir_a {
        bu.reverse();
        bp.reverse();
    }
    let at = |q: [f64; 2], d: f64| if k == 0 { [q[0] + d, q[1]] } else { [q[0], q[1] + d] };
    let a_end = at(a0.q, dir_a * pp);
    let b0 = bu.first().map_or([f64::NAN; 2], |w| w.q);
    let mut shift = 0.0;
    let mut guard = 0u32;
    while g2(&b0, k) + shift - g2(&a_end, k) > pp / 2.0 {
        shift -= pp;
        guard = guard.saturating_add(1);
        if guard > 1_000_000 {
            return Err(GeoErr("a band that never meets".into()));
        }
    }
    while g2(&b0, k) + shift - g2(&a_end, k) < -pp / 2.0 {
        shift += pp;
        guard = guard.saturating_add(1);
        if guard > 1_000_000 {
            return Err(GeoErr("a band that never meets".into()));
        }
    }
    let bq: Vec<Uv> = bu.iter().map(|w| Uv::plain(at(w.q, shift))).collect();
    let b_end = at(bq.first().map_or([f64::NAN; 2], |w| w.q), -dir_a * pp);
    let mut uv: Vec<Uv> = au.iter().map(|w| Uv::plain(w.q)).collect();
    uv.push(Uv::plain(a_end));
    uv.extend(bq);
    uv.push(Uv::plain(b_end));
    let mut p = a.p.clone();
    p.push(a.p.first().copied().unwrap_or([f64::NAN; 3]));
    let bp0 = bp.first().copied().unwrap_or([f64::NAN; 3]);
    p.extend(bp);
    p.push(bp0);
    Ok(UvLoop { p, uv, st: Vec::new(), outer: false })
}

/// Scale factors for (u, v) so the parameter plane is roughly isometric to the surface near the loops.
fn metric(s: &Surface, uv: &[[f64; 2]]) -> [f64; 2] {
    if let Some(m) = s.scale_uv {
        return m;
    }
    let (mut su, mut sv, mut n) = (0.0, 0.0, 0.0);
    let step = (uv.len() / 24).max(1);
    for &[u, v] in uv.iter().step_by(step) {
        let e = 1e-4 * (1.0 + u.abs());
        let e2 = 1e-4 * (1.0 + v.abs());
        let p = s.at(u, v);
        su += len(&sub(&s.at(u + e, v), &p)) / e;
        sv += len(&sub(&s.at(u, v + e2), &p)) / e2;
        n += 1.0;
    }
    [super::math::or(su / n, 1.0), super::math::or(sv / n, 1.0)]
}

fn pip(q: &[f64; 2], p: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = p.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (Some(a), Some(b)) = (p.get(i), p.get(j)) else {
            j = i;
            continue;
        };
        if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}

fn seg_dist(q: &[f64; 2], a: &[f64; 2], b: &[f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if super::math::falsy(l2) { 0.0 } else { max(0.0, min(1.0, ((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2)) };
    hypot2(q[0] - a[0] - t * dx, q[1] - a[1] - t * dy)
}

#[inline]
fn orient(a: &[f64; 2], b: &[f64; 2], c: &[f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Do segments p1–p2 and p3–p4 cross at a point inside both (not at an end)?
fn crosses(p1: &[f64; 2], p2: &[f64; 2], p3: &[f64; 2], p4: &[f64; 2]) -> bool {
    let (d1, d2, d3, d4) = (orient(p3, p4, p1), orient(p3, p4, p2), orient(p1, p2, p3), orient(p1, p2, p4));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Make each segment [a, b] an edge of the triangulation (counter-clockwise; changed in place) by flipping the diagonals
/// that cross it (Sloan 1993). Segments that cannot be recovered (overlapping input) are left as they are.
pub fn recover_edges(pp: &[[f64; 2]], tris: &mut [usize], segs: &[(usize, usize)]) {
    let ntri = tris.len() / 3;
    let pt = |k: usize| pp.get(k).copied().unwrap_or([f64::NAN; 2]);
    let key = |a: usize, b: usize| (a as u64).wrapping_mul(1_048_576).wrapping_add(b as u64);
    let mut owner: HashMap<u64, usize> = HashMap::new();
    let set_tri = |tris: &mut [usize], owner: &mut HashMap<u64, usize>, t: usize, a: usize, b: usize, c: usize| {
        let (b, c) = if orient(&pt(a), &pt(b), &pt(c)) < 0.0 { (c, b) } else { (b, c) };
        if let Some(s) = tris.get_mut(t.saturating_mul(3)..t.saturating_mul(3).saturating_add(3)) {
            s.copy_from_slice(&[a, b, c]);
        }
        owner.insert(key(a, b), t);
        owner.insert(key(b, c), t);
        owner.insert(key(c, a), t);
    };
    for t in 0..ntri {
        let (a, b, c) = (
            tris.get(t.saturating_mul(3)).copied(),
            tris.get(t.saturating_mul(3).saturating_add(1)).copied(),
            tris.get(t.saturating_mul(3).saturating_add(2)).copied(),
        );
        if let (Some(a), Some(b), Some(c)) = (a, b, c) {
            set_tri(tris, &mut owner, t, a, b, c);
        }
    }
    let third = |tris: &[usize], t: usize, a: usize, b: usize| -> Option<usize> {
        tris.get(t.saturating_mul(3)..t.saturating_mul(3).saturating_add(3))?.iter().copied().find(|&v| v != a && v != b)
    };
    for &(a, b) in segs {
        if a == b || owner.contains_key(&key(a, b)) || owner.contains_key(&key(b, a)) {
            continue;
        }
        // the edges crossing a–b
        let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
        for t in tris.as_chunks::<3>().0.iter() {
            for k in 0..3 {
                let (u, v) = (t.get(k).copied().unwrap_or(0), t.get(super::math::next(k, 3)).copied().unwrap_or(0));
                if u < v && u != a && u != b && v != a && v != b && crosses(&pt(a), &pt(b), &pt(u), &pt(v)) {
                    queue.push_back((u, v));
                }
            }
        }
        let mut guard: i64 = i64::try_from(queue.len()).unwrap_or(i64::MAX / 4).saturating_pow(2).saturating_add(50);
        while !queue.is_empty() {
            let go = guard > 0;
            guard = guard.saturating_sub(1);
            if !go {
                break;
            }
            let Some((c, d)) = queue.pop_front() else { break };
            let (Some(&t1), Some(&t2)) = (owner.get(&key(c, d)), owner.get(&key(d, c))) else { continue };
            let (Some(e), Some(f)) = (third(tris, t1, c, d), third(tris, t2, c, d)) else { continue };
            if !crosses(&pt(c), &pt(d), &pt(e), &pt(f)) {
                queue.push_back((c, d)); // not convex yet: later
                continue;
            }
            owner.remove(&key(c, d));
            owner.remove(&key(d, c));
            set_tri(tris, &mut owner, t1, c, f, e);
            set_tri(tris, &mut owner, t2, d, e, f);
            if e != a && e != b && f != a && f != b && crosses(&pt(a), &pt(b), &pt(e), &pt(f)) {
                queue.push_back((e, f));
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Tri {
    a: usize,
    b: usize,
    c: usize,
    cx: f64,
    cy: f64,
    r2: f64,
    dead: bool,
    mark: bool,
}

fn mk(p: &[[f64; 2]], a: usize, b: usize, c: usize) -> Tri {
    // counter-clockwise, with its circumcircle
    let g = |k: usize| p.get(k).copied().unwrap_or([f64::NAN; 2]);
    let (pa, pb, pc) = (g(a), g(b), g(c));
    let (b, c) = if (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]) < 0.0 { (c, b) } else { (b, c) };
    let ([ax, ay], [bx, by], [cx, cy]) = (g(a), g(b), g(c));
    let d = super::math::or(2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by)), 1e-300);
    let ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
    Tri { a, b, c, cx: ux, cy: uy, r2: (ax - ux) * (ax - ux) + (ay - uy) * (ay - uy), dead: false, mark: false }
}

/// Bowyer–Watson Delaunay triangulation of 2D points → [i, j, k, …] counter-clockwise. Triangles are indexed in a grid by
/// their circumcircle's box, so an insertion only tests the triangles whose circle can hold the new point.
pub fn delaunay(pp: &[[f64; 2]]) -> GR<Vec<usize>> {
    let n = pp.len();
    if n < 3 {
        return Ok(Vec::new());
    }
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in pp {
        if p[0] < lo[0] {
            lo[0] = p[0];
        }
        if p[1] < lo[1] {
            lo[1] = p[1];
        }
        if p[0] > hi[0] {
            hi[0] = p[0];
        }
        if p[1] > hi[1] {
            hi[1] = p[1];
        }
    }
    let d = super::math::or(max(hi[0] - lo[0], hi[1] - lo[1]), 1.0);
    let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    let mut pts = pp.to_vec();
    pts.extend_from_slice(&[[cx - 20.0 * d, cy - d], [cx + 20.0 * d, cy - d], [cx, cy + 20.0 * d]]);
    let gsz = max(1.0, min(256.0, (usize_f64(n) / 2.0).sqrt().ceil()));
    let gn = crate::util::f64_index(gsz, 257).unwrap_or(1);
    let cw = super::math::or((hi[0] - lo[0]) / gsz, 1.0);
    let ch = super::math::or((hi[1] - lo[1]) / gsz, 1.0);
    let mut cells: Vec<Vec<usize>> = vec![Vec::new(); gn.saturating_mul(gn)];
    let mut big: Vec<usize> = Vec::new();
    let mut ts: Vec<Tri> = Vec::new();
    let gm1 = gsz - 1.0;
    let cell_of =
        |x: f64, y: f64| -> (f64, f64) { (min(gm1, max(0.0, ((x - lo[0]) / cw).floor())), min(gm1, max(0.0, ((y - lo[1]) / ch).floor()))) };
    let add_t = |t: Tri, ts: &mut Vec<Tri>, cells: &mut Vec<Vec<usize>>, big: &mut Vec<usize>| {
        let k = ts.len();
        ts.push(t);
        let r = t.r2.sqrt();
        let (x0, y0) = cell_of(t.cx - r, t.cy - r);
        let (x1, y1) = cell_of(t.cx + r, t.cy + r);
        if (x1 - x0 + 1.0) * (y1 - y0 + 1.0) > 64.0 {
            big.push(k);
            return;
        }
        // NaN bounds: the loops run zero times (the triangle is in no list)
        let (Some(x0), Some(x1), Some(y0), Some(y1)) = (
            crate::util::f64_index(x0, gn),
            crate::util::f64_index(x1, gn),
            crate::util::f64_index(y0, gn),
            crate::util::f64_index(y1, gn),
        ) else {
            return;
        };
        for x in x0..=x1 {
            for y in y0..=y1 {
                if let Some(c) = cells.get_mut(y.saturating_mul(gn).saturating_add(x)) {
                    c.push(k);
                }
            }
        }
    };
    add_t(mk(&pts, n, n.saturating_add(1), n.saturating_add(2)), &mut ts, &mut cells, &mut big);
    let colf = |p: &[f64; 2]| ((p[0] - lo[0]) / cw).floor();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let (pa, pb) = (pts.get(a).copied().unwrap_or([f64::NAN; 2]), pts.get(b).copied().unwrap_or([f64::NAN; 2]));
        let d1 = colf(&pa) - colf(&pb);
        let v = if super::math::falsy(d1) { pa[1] - pb[1] } else { d1 };
        if v < 0.0 {
            std::cmp::Ordering::Less
        } else if v > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    for i in order {
        let [x, y] = pts.get(i).copied().unwrap_or([f64::NAN; 2]);
        let (gx, gy) = cell_of(x, y);
        let (Some(gx), Some(gy)) = (crate::util::f64_index(gx, gn), crate::util::f64_index(gy, gn)) else {
            // cells[NaN] is undefined: the JS reads .length of it
            return Err(type_error_read("undefined", "length"));
        };
        let ci = gy.saturating_mul(gn).saturating_add(gx);
        let mut bad: Vec<usize> = Vec::new();
        let test = |list: &mut Vec<usize>, ts: &mut Vec<Tri>, bad: &mut Vec<usize>| {
            let mut k = 0usize;
            while k < list.len() {
                let Some(&ti) = list.get(k) else { break };
                let Some(t) = ts.get_mut(ti) else {
                    k = k.saturating_add(1);
                    continue;
                };
                if t.dead {
                    list.swap_remove(k);
                    continue;
                }
                let (dx, dy) = (x - t.cx, y - t.cy);
                if !t.mark && dx * dx + dy * dy < t.r2 * (1.0 + 1e-12) {
                    t.mark = true;
                    bad.push(ti);
                }
                k = k.saturating_add(1);
            }
        };
        if let Some(cand) = cells.get_mut(ci) {
            test(cand, &mut ts, &mut bad);
        }
        test(&mut big, &mut ts, &mut bad);
        // the boundary of the cavity: edges of exactly one bad triangle, in the JS Map's insertion order
        let mut edges: Vec<(u64, usize, usize, bool)> = Vec::new();
        let mut at: HashMap<u64, usize> = HashMap::new();
        for &ti in &bad {
            let Some(t) = ts.get_mut(ti) else { continue };
            t.dead = true;
            let (ta, tb, tc) = (t.a, t.b, t.c);
            for (a, b) in [(ta, tb), (tb, tc), (tc, ta)] {
                let k = if a < b {
                    (a as u64).wrapping_mul(1_048_576).wrapping_add(b as u64)
                } else {
                    (b as u64).wrapping_mul(1_048_576).wrapping_add(a as u64)
                };
                if let Some(pos) = at.remove(&k) {
                    if let Some(e) = edges.get_mut(pos) {
                        e.3 = false;
                    }
                } else {
                    at.insert(k, edges.len());
                    edges.push((k, a, b, true));
                }
            }
        }
        for &(_, a, b, alive) in &edges {
            if alive {
                add_t(mk(&pts, a, b, i), &mut ts, &mut cells, &mut big);
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = vec![false; ts.len()];
    for list in cells.iter().chain(std::iter::once(&big)) {
        for &ti in list {
            let Some(t) = ts.get(ti) else { continue };
            if t.dead || seen.get(ti).copied().unwrap_or(true) {
                continue;
            }
            if let Some(s) = seen.get_mut(ti) {
                *s = true;
            }
            if t.a < n && t.b < n && t.c < n {
                out.extend_from_slice(&[t.a, t.b, t.c]);
            }
        }
    }
    Ok(out)
}
