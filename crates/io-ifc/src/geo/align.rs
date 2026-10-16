//! geo-align.js — IFC4X3 alignment geometry (IFC4.3 ADD2 §8.9 Geometry resource: IfcCurveSegment, IfcGradientCurve,
//! IfcSegmentedReferenceCurve, IfcClothoid, IfcPolynomialCurve; IfcAxis2PlacementLinear, IfcPointByDistanceExpression;
//! §8.8 IfcSectionedSolidHorizontal).
//!
//! A curve segment takes the part of its ParentCurve from SegmentStart for SegmentLength (lengths along the curve; a
//! negative length runs backwards) and places it so the parent's point at SegmentStart is at the segment's Placement,
//! running along its x axis. Parent curves: IfcLine, IfcCircle, IfcClothoid (heading s²/2A², integrated numerically),
//! IfcPolynomialCurve; other spirals are drawn as their chord (noted).

use std::rc::Rc;

use super::ctx::{GR, GeoCtx, list_or_empty, numd, refs, stack_overflow};
use super::curve::curve_points;
use super::math::{M4, V3, add, apply, apply_v, cross, dot, frame, invert, len, mul, norm, scale, sub};
use super::mesh::Mesh;
use super::profile::profile_shapes;
use crate::jsmath::{JsMath, hypot2, max, min, sign};
use crate::model::{Id, id_key};

/// A sampled alignment curve with the horizontal distance of each point.
#[derive(Clone, Debug, Default)]
pub struct SegCurve {
    pub pts: Vec<V3>,
    pub dist: Vec<f64>,
}

/// How deep alignment curves may nest through each other (a loop would exhaust the JS call stack).
const MAX_SEG_DEPTH: u32 = 200;

/// Is this composite made of IFC4X3 curve segments?
pub fn is_segmented(g: &GeoCtx<'_>, id: Option<Id>) -> GR<bool> {
    let t = g.t(id);
    if t == Some("IFCGRADIENTCURVE") || t == Some("IFCSEGMENTEDREFERENCECURVE") {
        return Ok(true);
    }
    if t != Some("IFCCOMPOSITECURVE") {
        return Ok(false);
    }
    let segs = refs(g.a(id, "Segments")?);
    Ok(segs.first().is_some_and(|&s| g.t(Some(s)) == Some("IFCCURVESEGMENT")))
}

enum Parent {
    Line { o: V3, d: V3 },
    Circle { m: M4, r: f64 },
    Clothoid { m: M4, k: f64 },
    Poly { m: M4, cx: Vec<f64>, cy: Vec<f64>, cz: Vec<f64> },
}

impl Parent {
    fn at(&self, s: f64) -> V3 {
        match self {
            Parent::Line { o, d } => add(o, &scale(d, s)),
            Parent::Circle { m, r } => apply(m, r * (s / r).jcos(), r * (s / r).jsin(), 0.0),
            Parent::Clothoid { m, k } => {
                let n = max(8.0, min(2000.0, (s.abs() / 0.5).ceil()) * 2.0);
                let h = s / n;
                let (mut x, mut y) = (0.0, 0.0);
                // for (i = 0; i <= n; i++) — n is at most 4000 (NaN: no steps)
                let steps = if n >= 0.0 { crate::util::f64_index(n, 100_000).map_or(0, |v| v.saturating_add(1)) } else { 0 };
                for i in 0..steps {
                    let fi = crate::util::usize_f64(i);
                    let w = if i == 0 || fi == n {
                        1.0
                    } else if i % 2 == 1 {
                        4.0
                    } else {
                        2.0
                    };
                    let u = fi * h;
                    let th = k * u * u;
                    x += w * th.jcos();
                    y += w * th.jsin();
                }
                apply(m, x * h / 3.0, y * h / 3.0, 0.0)
            }
            Parent::Poly { m, cx, cy, cz } => {
                let poly = |c: &[f64], u: f64| c.iter().enumerate().fold(0.0, |acc, (i, a)| acc + a * u.jpow(crate::util::usize_f64(i)));
                apply(m, poly(cx, s), poly(cy, s), poly(cz, s))
            }
        }
    }
}

/// A parent curve in its own coordinates: p(s) for s along it (length measure), 2D or 3D.
fn parent_curve(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Option<Parent>> {
    let t = g.t(id);
    match t.unwrap_or("") {
        "IFCLINE" => {
            let o = g.point(g.ra(id, "Pnt")?)?;
            let d = norm(&g.vector(g.ra(id, "Dir")?)?);
            Ok(Some(Parent::Line { o, d }))
        }
        "IFCCIRCLE" => {
            let m = g.axis2(g.ra(id, "Position")?)?;
            let r = super::math::or(g.na(id, "Radius", 1.0)?, 1.0);
            Ok(Some(Parent::Circle { m, r }))
        }
        "IFCCLOTHOID" => {
            let m = g.axis2(g.ra(id, "Position")?)?;
            let a = super::math::or(g.na(id, "ClothoidConstant", 1.0)?, 1.0);
            // heading θ(s) = sign(A) s² / (2A²); x = ∫cos θ, y = ∫sin θ (Simpson's rule, fine steps)
            Ok(Some(Parent::Clothoid { m, k: sign(a) / (2.0 * a * a) }))
        }
        "IFCPOLYNOMIALCURVE" => {
            let m = g.axis2(g.ra(id, "Position")?)?;
            let c =
                |g: &GeoCtx<'_>, n: &str| -> GR<Vec<f64>> { Ok(list_or_empty(g.a(id, n)?)?.iter().map(|v| numd(Some(v), 0.0)).collect()) };
            let (cx, cy, cz) = (c(g, "CoefficientsX")?, c(g, "CoefficientsY")?, c(g, "CoefficientsZ")?);
            Ok(Some(Parent::Poly { m, cx, cy, cz }))
        }
        other => {
            if !other.is_empty() {
                let p = g.pretty(other);
                g.note(format!("{p} alignment segments are drawn as straight lines"));
            }
            Ok(None)
        }
    }
}

/// Points of one IfcCurveSegment, placed: the parent's point at SegmentStart lands on Placement, heading along its x.
fn segment_points(g: &mut GeoCtx<'_>, seg: Option<Id>) -> GR<Vec<V3>> {
    let start = numd(g.a(seg, "SegmentStart")?, 0.0);
    let length = numd(g.a(seg, "SegmentLength")?, 0.0);
    let p = g.axis2(g.ra(seg, "Placement")?)?;
    if super::math::falsy(length) {
        return Ok(Vec::new());
    }
    let pc = g.ra(seg, "ParentCurve")?;
    let f = parent_curve(g, pc)?;
    let n = max(2.0, min(400.0, (length.abs() / 2.0).ceil()));
    let Some(f) = f else { return Ok(vec![apply(&p, 0.0, 0.0, 0.0), apply(&p, length.abs(), 0.0, 0.0)]) };
    let sgn = sign(length);
    let p0 = f.at(start);
    let e = max(1e-6, length.abs() * 1e-6);
    let mut tx = norm(&scale(&sub(&f.at(start + sgn * e), &p0), 1.0));
    if super::math::falsy(len(&tx)) {
        tx = [1.0, 0.0, 0.0];
    }
    let z = [0.0, 0.0, 1.0];
    let mut y = norm(&cross(&z, &tx));
    if super::math::falsy(len(&y)) {
        y = [0.0, 1.0, 0.0];
    }
    let f0 = frame(&p0, &tx, &y, &cross(&tx, &y));
    let t = mul(&p, &invert(&f0).unwrap_or(f0));
    let cnt = super::curve::count(n + 1.0)?;
    let mut out = Vec::with_capacity(cnt);
    for i in 0..cnt {
        out.push(apply_v(&t, &f.at(start + length * crate::util::usize_f64(i) / n)));
    }
    Ok(out)
}

fn horizontal_of(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Vec<V3>> {
    let mut pts: Vec<V3> = Vec::new();
    for seg in refs(g.a(id, "Segments")?) {
        let s = if g.t(Some(seg)) == Some("IFCCURVESEGMENT") { segment_points(g, Some(seg))? } else { curve_points(g, Some(seg), 0)? };
        for p in s {
            if pts.last().is_none_or(|l| len(&sub(l, &p)) > 1e-9) {
                pts.push([p[0], p[1], super::math::or(p[2], 0.0)]);
            }
        }
    }
    Ok(pts)
}

fn stations(pts: &[V3]) -> Vec<f64> {
    let mut d = Vec::with_capacity(pts.len().max(1));
    d.push(0.0);
    for w in pts.windows(2) {
        if let [a, b] = w {
            let prev = d.last().copied().unwrap_or(0.0);
            d.push(prev + hypot2(b[0] - a[0], b[1] - a[1]));
        }
    }
    d
}

/// Sampled alignment curve with the horizontal distance of each point (cached per curve).
pub fn segmented_curve(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Rc<SegCurve>> {
    let key = id.map_or(u64::MAX, id_key);
    if let Some(c) = g.seg_cache.get(&key) {
        return Ok(c.clone());
    }
    if g.seg_depth >= MAX_SEG_DEPTH {
        return Err(stack_overflow());
    }
    g.seg_depth = g.seg_depth.saturating_add(1);
    let r = segmented_inner(g, id);
    g.seg_depth = g.seg_depth.saturating_sub(1);
    let out = Rc::new(r?);
    g.seg_cache.insert(key, out.clone());
    Ok(out)
}

fn segmented_inner(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<SegCurve> {
    let t = g.t(id);
    if t == Some("IFCGRADIENTCURVE") || t == Some("IFCSEGMENTEDREFERENCECURVE") {
        let bc = g.ra(id, "BaseCurve")?;
        let base: SegCurve = if t == Some("IFCSEGMENTEDREFERENCECURVE") {
            (*segmented_curve(g, bc)?).clone()
        } else {
            let h = horizontal_of(g, bc)?;
            let dist = stations(&h);
            SegCurve { pts: h, dist }
        };
        if t == Some("IFCSEGMENTEDREFERENCECURVE") {
            g.note("Cant (superelevation) is not applied yet");
            return Ok(base);
        }
        // vertical profile: points (distance along, height), sorted by distance (stable)
        let mut prof: Vec<[f64; 2]> = horizontal_of(g, id)?.iter().map(|p| [p[0], p[1]]).collect();
        prof.sort_by(|a, b| {
            let d = a[0] - b[0];
            if d < 0.0 {
                std::cmp::Ordering::Less
            } else if d > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        let z_at = |d: f64| -> f64 {
            let (Some(first), Some(lastp)) = (prof.first(), prof.last()) else { return 0.0 };
            if d <= first[0] {
                return first[1];
            }
            let mut lo = 0usize;
            let mut hi = prof.len().saturating_sub(1);
            if d >= lastp[0] {
                return lastp[1];
            }
            while hi.saturating_sub(lo) > 1 {
                let m = lo.saturating_add(hi) / 2;
                if prof.get(m).is_some_and(|q| q[0] <= d) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            let (pl, ph) = (prof.get(lo).copied().unwrap_or([f64::NAN; 2]), prof.get(hi).copied().unwrap_or([f64::NAN; 2]));
            let f = (d - pl[0]) / super::math::or(ph[0] - pl[0], 1.0);
            pl[1] + (ph[1] - pl[1]) * f
        };
        let pts = base.pts.iter().enumerate().map(|(i, p)| [p[0], p[1], z_at(base.dist.get(i).copied().unwrap_or(f64::NAN))]).collect();
        return Ok(SegCurve { pts, dist: base.dist });
    }
    let h = horizontal_of(g, id)?;
    let dist = stations(&h);
    Ok(SegCurve { pts: h, dist })
}

/// The point and unit tangent at a horizontal distance along a curve (alignment curves; others by arc length).
pub fn point_along(g: &mut GeoCtx<'_>, id: Option<Id>, d: f64) -> GR<(V3, V3)> {
    let c: Rc<SegCurve> = if is_segmented(g, id)? {
        segmented_curve(g, id)?
    } else {
        let p = curve_points(g, id, 0)?;
        let dist = stations(&p);
        Rc::new(SegCurve { pts: p, dist })
    };
    let (pts, dist) = (&c.pts, &c.dist);
    if pts.len() < 2 {
        return Ok((pts.first().copied().unwrap_or([0.0; 3]), [1.0, 0.0, 0.0]));
    }
    let mut i = 1usize;
    while i < pts.len().saturating_sub(1) && dist.get(i).is_some_and(|&x| x < d) {
        i = i.saturating_add(1);
    }
    let di = |k: usize| dist.get(k).copied().unwrap_or(f64::NAN);
    let pi = |k: usize| pts.get(k).copied().unwrap_or([f64::NAN; 3]);
    let im = i.saturating_sub(1);
    let f = (d - di(im)) / super::math::or(di(i) - di(im), 1.0);
    let p = add(&pi(im), &scale(&sub(&pi(i), &pi(im)), f));
    Ok((p, norm(&sub(&pi(i), &pi(im)))))
}

/// IfcAxis2PlacementLinear → matrix: the station's point plus offsets, x along the curve (level), z up (or as given).
pub fn linear_placement(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Option<M4>> {
    let loc = g.ra(id, "Location")?;
    if g.t(loc) != Some("IFCPOINTBYDISTANCEEXPRESSION") {
        return Ok(None);
    }
    let basis = g.ra(loc, "BasisCurve")?;
    let d = g.na(loc, "DistanceAlong", 0.0)?;
    let (p, t) = point_along(g, basis, d)?;
    let th = norm(&[t[0], t[1], 0.0]);
    let left = [-th[1], th[0], 0.0];
    let o = add(
        &add(&add(&p, &scale(&left, g.na(loc, "OffsetLateral", 0.0)?)), &[0.0, 0.0, g.na(loc, "OffsetVertical", 0.0)?]),
        &scale(&th, g.na(loc, "OffsetLongitudinal", 0.0)?),
    );
    let z_axis = g.dirv(g.ra(id, "Axis")?, [0.0, 0.0, 1.0])?;
    let rf = g.dir(g.ra(id, "RefDirection")?, None)?;
    let x0 = rf.unwrap_or(th);
    let mut x = norm(&sub(&x0, &scale(&z_axis, dot(&x0, &z_axis))));
    if super::math::falsy(len(&x)) {
        x = th;
    }
    Ok(Some(frame(&o, &x, &cross(&z_axis, &x), &z_axis)))
}

/// IfcSectionedSolidHorizontal: the cross sections placed across the directrix at their stations, lofted between.
pub fn sectioned_solid(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let dir = g.ra(id, "Directrix")?;
    let mut profs = Vec::new();
    for p in refs(g.a(id, "CrossSections")?) {
        if let Some(s) = profile_shapes(g, Some(p), 0)?.into_iter().next() {
            profs.push(s);
        }
    }
    let pos = refs(g.a(id, "CrossSectionPositions")?);
    let mut m = Mesh::new();
    if profs.len() < 2 || pos.len() != profs.len() {
        g.note("A sectioned solid without matching sections is not drawn");
        return Ok(m);
    }
    let mut st = Vec::with_capacity(pos.len());
    for q in &pos {
        let l = g.ra(Some(*q), "Location")?;
        st.push(g.na(l, "DistanceAlong", 0.0)?);
    }
    let c = segmented_curve(g, dir)?;
    // the frame at a station: x to the left (level), y up, z along the alignment
    let mut rings: Vec<(Vec<V3>, f64)> = Vec::new();
    for k in 0..profs.len().saturating_sub(1) {
        let (Some(pa), Some(pb)) = (profs.get(k), profs.get(k.saturating_add(1))) else { continue };
        let (a, b) = (&pa.outer, &pb.outer);
        if a.len() != b.len() {
            g.note("Sections with different numbers of points are lofted from the first one");
        }
        let d0 = st.get(k).copied().unwrap_or(f64::NAN);
        let d1 = st.get(k.saturating_add(1)).copied().unwrap_or(f64::NAN);
        let mut ds = vec![d0];
        ds.extend(c.dist.iter().copied().filter(|&d| d > d0 && d < d1));
        ds.push(d1);
        for d in ds {
            if rings.last().is_some_and(|r| r.1 == d) {
                continue;
            }
            let f = if d1 > d0 { (d - d0) / (d1 - d0) } else { 0.0 };
            let (pt, t) = point_along(g, dir, d)?;
            let th = norm(&[t[0], t[1], 0.0]);
            let fx = [-th[1], th[0], 0.0];
            let fy = [0.0, 0.0, 1.0];
            let ring = a
                .iter()
                .enumerate()
                .map(|(i, pa)| {
                    let pb = b.get(i).unwrap_or(pa);
                    let x = pa[0] + (pb[0] - pa[0]) * f;
                    let y = pa[1] + (pb[1] - pa[1]) * f;
                    add(&pt, &add(&scale(&fx, x), &scale(&fy, y)))
                })
                .collect();
            rings.push((ring, d));
        }
    }
    if rings.len() < 2 {
        return Ok(m);
    }
    let n = rings.first().map_or(0, |r| r.0.len());
    for w in rings.windows(2) {
        let [(a, _), (b, _)] = w else { continue };
        for i in 0..n {
            let j = super::math::next(i, n);
            let q = |r: &Vec<V3>, k: usize| r.get(k).copied();
            // ring points past a shorter ring's end are undefined in the JS: the polygon then has fewer points
            let quad: Vec<V3> = [q(a, i), q(a, j), q(b, j), q(b, i)].into_iter().flatten().collect();
            if quad.len() == 4 {
                m.polygon(&[quad]);
            } else {
                return Err(super::ctx::type_error_read("undefined", "0"));
            }
        }
    }
    if let Some((r0, _)) = rings.first() {
        let mut r = r0.clone();
        r.reverse();
        m.polygon(&[r]);
    }
    if let Some((rl, _)) = rings.last() {
        m.polygon(std::slice::from_ref(rl));
    }
    if m.volume() < 0.0 {
        m.flip();
    }
    Ok(m)
}
