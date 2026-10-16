//! geo-curve.js — IFC curves as point lists (IFC4 ADD2 TC1 §8.9 Geometry resource): IfcPolyline, IfcIndexedPolyCurve
//! (line and arc segments), IfcCompositeCurve (+ IfcCompositeCurveSegment SameSense), IfcTrimmedCurve (by parameter or
//! by point, SenseAgreement) of IfcLine / IfcCircle / IfcEllipse / any bounded curve, full IfcCircle and IfcEllipse,
//! IfcBSplineCurveWithKnots and IfcRationalBSplineCurveWithKnots (de Boor), IfcOffsetCurve2D (straight offsets),
//! IfcSurfaceCurve / IfcIntersectionCurve / IfcSeamCurve (their Curve3D), and IFC4X3 alignment curves (align.rs).
//!
//! Conic trimming parameters are in the project's plane angle unit (g.angle_unit), lines' in length along the vector.

use super::align::{is_segmented, segmented_curve};
use super::ctx::{GR, GeoCtx, GeoErr, arr, bool_, enm, idx, list_or_empty, num, numd, refs};
use super::math::{V3, add, apply, dot, len, norm, scale, sub};
use crate::jsmath::{JsMath, hypot2, hypot3, max};
use crate::model::Id;
use crate::value::Value;

pub const CIRCLE_SEGMENTS: f64 = 36.0;
const TAU: f64 = 2.0 * std::f64::consts::PI;

/// The most points one tessellated curve may have (the JS would run out of memory far above real files).
pub const MAX_COUNT: f64 = 10_000_000.0;

/// `Math.max(radius > 0 ? 2 : 1, ceil(|sweep| / 2π · 36))` (a JS number: NaN stays NaN).
pub fn arc_segments(sweep: f64, radius: f64) -> f64 {
    let n = (sweep.abs() / TAU * CIRCLE_SEGMENTS).ceil();
    max(if radius > 0.0 { 2.0 } else { 1.0 }, n)
}

/// A loop bound held as a JS number → how many times `for (i = 0; i < n; i++)` runs (NaN: none); a count past
/// MAX_COUNT throws (the JS would hang or run out of memory).
pub fn count(n: f64) -> GR<usize> {
    if n.is_nan() || n <= 0.0 {
        return Ok(0);
    }
    if n > MAX_COUNT {
        return Err(GeoErr("Invalid array length".into()));
    }
    Ok(crate::util::f64_index(n.ceil(), usize::MAX).unwrap_or(0))
}

/// `pts[k]` for a JS-number index.
pub fn at_f<T>(pts: &[T], k: f64) -> Option<&T> {
    pts.get(crate::util::f64_index(k, pts.len())?)
}

pub fn curve_points(g: &mut GeoCtx<'_>, id: Option<Id>, depth: u32) -> GR<Vec<V3>> {
    if id.is_none() || depth > 32 {
        return Ok(Vec::new());
    }
    let t = g.t(id);
    match t.unwrap_or("") {
        "IFCPOLYLINE" => {
            let mut out = Vec::new();
            for p in refs(g.a(id, "Points")?) {
                out.push(g.point(Some(p))?);
            }
            Ok(out)
        }
        "IFCINDEXEDPOLYCURVE" => indexed_poly_curve(g, id),
        "IFCCOMPOSITECURVE" | "IFCCOMPOSITECURVEONSURFACE" | "IFCOUTERBOUNDARYCURVE" | "IFCBOUNDARYCURVE" => {
            // IFC4X3 alignments: composites of placed curve segments (align.rs)
            if is_segmented(g, id)? {
                return Ok(segmented_curve(g, id)?.pts.clone());
            }
            composite_curve(g, id, depth)
        }
        "IFCGRADIENTCURVE" | "IFCSEGMENTEDREFERENCECURVE" => Ok(segmented_curve(g, id)?.pts.clone()),
        "IFCTRIMMEDCURVE" => trimmed_curve(g, id, depth),
        "IFCCIRCLE" | "IFCELLIPSE" => conic(g, id, 0.0, TAU, true),
        "IFCBSPLINECURVEWITHKNOTS" | "IFCRATIONALBSPLINECURVEWITHKNOTS" => bspline(g, id),
        "IFCOFFSETCURVE2D" => offset_curve_2d(g, id, depth),
        // curves on surfaces carry their 3D form (IFC4 §8.9.3.71 IfcSurfaceCurve): draw that one
        "IFCSURFACECURVE" | "IFCINTERSECTIONCURVE" | "IFCSEAMCURVE" => {
            let c = g.ra(id, "Curve3D")?;
            curve_points(g, c, depth.saturating_add(1))
        }
        "IFCLINE" => {
            g.note("An unbounded line is used as a curve; drawn one unit long");
            Ok(vec![line_point(g, id, 0.0)?, line_point(g, id, 1.0)?])
        }
        other => {
            if !other.is_empty() {
                let p = g.pretty(other);
                g.note(format!("{p} curves are not drawn yet"));
            }
            Ok(Vec::new())
        }
    }
}

/// Points of an IfcCartesianPointList2D/3D (1-based indexes in the segments).
pub fn point_list(g: &GeoCtx<'_>, id: Option<Id>) -> GR<Vec<V3>> {
    let Some(c) = arr(g.a(id, "CoordList")?) else { return Ok(Vec::new()) };
    let mut out = Vec::with_capacity(c.len());
    for p in c {
        out.push([numd(idx(Some(p), 0)?, 0.0), numd(idx(Some(p), 1)?, 0.0), numd(idx(Some(p), 2)?, 0.0)]);
    }
    Ok(out)
}

fn indexed_poly_curve(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Vec<V3>> {
    let pts = point_list(g, g.ra(id, "Points")?)?;
    let segs = match arr(g.a(id, "Segments")?) {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(pts),
    };
    let mut out: Vec<V3> = Vec::new();
    let push = |out: &mut Vec<V3>, p: V3| {
        if out.last().is_none_or(|l| l[0] != p[0] || l[1] != p[1] || l[2] != p[2]) {
            out.push(p);
        }
    };
    for s in segs {
        let (kind, inner) = match s {
            Value::Typed(t) => (Some(&*t.0), Some(&t.1)),
            _ => (None, None),
        };
        let ix: Vec<f64> = match inner {
            Some(Value::List(l)) => l.iter().map(|v| numd(Some(v), 1.0) - 1.0).collect(),
            _ => Vec::new(),
        };
        if kind == Some("IFCARCINDEX") && ix.len() == 3 {
            let g3 = |k: usize| ix.get(k).and_then(|&k| at_f(&pts, k)).copied();
            for p in arc3(g3(0), g3(1), g3(2))? {
                push(&mut out, p);
            }
        } else {
            for &k in &ix {
                if let Some(p) = at_f(&pts, k) {
                    push(&mut out, *p);
                }
            }
        }
    }
    Ok(out)
}

/// The circular arc through three points (start, middle, end), tessellated; straight when they are collinear.
pub fn arc3(a: Option<V3>, b: Option<V3>, c: Option<V3>) -> GR<Vec<V3>> {
    let (Some(a), Some(b), Some(c)) = (a, b, c) else { return Ok([a, b, c].into_iter().flatten().collect()) };
    // circle centre in the plane of the three points
    let ab = sub(&b, &a);
    let ac = sub(&c, &a);
    let n = [ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0]];
    let nn = dot(&n, &n);
    if nn < 1e-18 * max(1.0, dot(&ab, &ab) * dot(&ac, &ac)) {
        return Ok(vec![a, c]);
    }
    let t1 = scale(&[ac[1] * n[2] - ac[2] * n[1], ac[2] * n[0] - ac[0] * n[2], ac[0] * n[1] - ac[1] * n[0]], dot(&ab, &ab));
    let t2 = scale(&[n[1] * ab[2] - n[2] * ab[1], n[2] * ab[0] - n[0] * ab[2], n[0] * ab[1] - n[1] * ab[0]], dot(&ac, &ac));
    let ctr = add(&a, &scale(&add(&t1, &t2), 1.0 / (2.0 * nn)));
    let r = len(&sub(&a, &ctr));
    let u = norm(&sub(&a, &ctr));
    let w = norm(&n);
    let v = [w[1] * u[2] - w[2] * u[1], w[2] * u[0] - w[0] * u[2], w[0] * u[1] - w[1] * u[0]];
    let ang = |p: &V3| {
        let d = sub(p, &ctr);
        dot(&d, &v).jatan2(dot(&d, &u))
    };
    let mut tc = ang(&c);
    if tc < 0.0 {
        tc += TAU;
    }
    let sweep = tc; // counter-clockwise about n from a to c passes b by construction of n
    let segs = arc_segments(sweep, r);
    let mut out = vec![a];
    let ns = count(segs)?;
    for i in 1..ns {
        let th = sweep * crate::util::usize_f64(i) / segs;
        out.push(add(&ctr, &add(&scale(&u, r * th.jcos()), &scale(&v, r * th.jsin()))));
    }
    out.push(c);
    Ok(out)
}

fn composite_curve(g: &mut GeoCtx<'_>, id: Option<Id>, depth: u32) -> GR<Vec<V3>> {
    let mut out: Vec<V3> = Vec::new();
    let d1 = depth.saturating_add(1);
    for seg in refs(g.a(id, "Segments")?) {
        let st = g.t(Some(seg));
        let mut same = true;
        let mut pts = if st == Some("IFCCOMPOSITECURVESEGMENT") || st == Some("IFCREPARAMETRISEDCOMPOSITECURVESEGMENT") {
            let pc = g.ra(Some(seg), "ParentCurve")?;
            let p = curve_points(g, pc, d1)?;
            same = bool_(g.a(Some(seg), "SameSense")?, Some(true)).unwrap_or(true);
            p
        } else if st == Some("IFCCURVESEGMENT") {
            g.note("Alignment curve segments are drawn as straight lines");
            let pc = g.ra(Some(seg), "ParentCurve")?;
            curve_points(g, pc, d1)?
        } else {
            curve_points(g, Some(seg), d1)?
        };
        if !same {
            pts.reverse();
        }
        for p in pts {
            if let Some(l) = out.last()
                && (l[0] - p[0]).abs() < 1e-9
                && (l[1] - p[1]).abs() < 1e-9
                && (l[2] - p[2]).abs() < 1e-9
            {
                continue;
            }
            out.push(p);
        }
    }
    Ok(out)
}

pub fn line_point(g: &GeoCtx<'_>, id: Option<Id>, t: f64) -> GR<V3> {
    let p = g.point(g.ra(id, "Pnt")?)?;
    let d = g.vector(g.ra(id, "Dir")?)?;
    Ok(add(&p, &scale(&d, t)))
}

/// A conic between parameters a0 and a1 (radians), counter-clockwise; `full` closes it without repeating the start.
fn conic(g: &mut GeoCtx<'_>, id: Option<Id>, a0: f64, a1: f64, full: bool) -> GR<Vec<V3>> {
    let circ = g.t(id) == Some("IFCCIRCLE");
    let m = g.axis2(g.ra(id, "Position")?)?;
    let r1 = g.na(id, if circ { "Radius" } else { "SemiAxis1" }, 0.0)?;
    let r2 = if circ { r1 } else { g.na(id, "SemiAxis2", 0.0)? };
    let sweep = a1 - a0;
    let n = arc_segments(sweep, max(r1, r2));
    // for (i = 0; i <= last; i++) with last = full ? n − 1 : n
    let last = if full { n - 1.0 } else { n };
    let cnt = count(last + 1.0)?;
    let mut out = Vec::with_capacity(cnt);
    for i in 0..cnt {
        let th = a0 + sweep * crate::util::usize_f64(i) / n;
        out.push(apply(&m, r1 * th.jcos(), r2 * th.jsin(), 0.0));
    }
    Ok(out)
}

/// Parameter of a point on a curve (conic → radians, line → length along the vector).
fn param_of(g: &mut GeoCtx<'_>, curve: Option<Id>, p: &V3) -> GR<Option<f64>> {
    let t = g.t(curve);
    if t == Some("IFCCIRCLE") || t == Some("IFCELLIPSE") {
        let circ = t == Some("IFCCIRCLE");
        let m = g.axis2(g.ra(curve, "Position")?)?;
        let d = sub(p, &[m[12], m[13], m[14]]);
        let r1 = super::math::or(g.na(curve, if circ { "Radius" } else { "SemiAxis1" }, 1.0)?, 1.0);
        let r2 = if circ { r1 } else { super::math::or(g.na(curve, "SemiAxis2", 1.0)?, 1.0) };
        return Ok(Some((dot(&d, &[m[4], m[5], m[6]]) / r2).jatan2(dot(&d, &[m[0], m[1], m[2]]) / r1)));
    }
    if t == Some("IFCLINE") {
        let o = g.point(g.ra(curve, "Pnt")?)?;
        let v = g.vector(g.ra(curve, "Dir")?)?;
        let l2 = super::math::or(dot(&v, &v), 1.0);
        return Ok(Some(dot(&sub(p, &o), &v) / l2));
    }
    Ok(None)
}

#[derive(Default, Clone, Copy)]
struct Trim {
    par: Option<f64>,
    pt: Option<V3>,
}

/// The most turns a trimmed conic's end parameter is moved to pass its start (the JS loops forever past it).
const MAX_TURNS: u32 = 1_000_000;

fn trimmed_curve(g: &mut GeoCtx<'_>, id: Option<Id>, depth: u32) -> GR<Vec<V3>> {
    let basis = g.ra(id, "BasisCurve")?;
    let bt = g.t(basis);
    let sense = bool_(g.a(id, "SenseAgreement")?, Some(true)).unwrap_or(true);
    let master = enm(g.a(id, "MasterRepresentation")?).map(str::to_string);
    let pick = |g: &GeoCtx<'_>, trim: Option<&Value>| -> GR<Trim> {
        let list: Vec<&Value> = match trim {
            Some(Value::List(l)) => l.iter().collect(),
            Some(v) => vec![v],
            None => vec![],
        };
        let mut par: Option<f64> = None;
        let mut pt: Option<V3> = None;
        for v in list {
            match v {
                // `par = g.num(v.value)`: a non-number makes it null again
                Value::Typed(t) if &*t.0 == "IFCPARAMETERVALUE" => par = num(Some(&t.1)),
                Value::Num(n) => par = Some(*n),
                Value::Ref(r) if g.t(Some(*r)) == Some("IFCCARTESIANPOINT") => pt = Some(g.point(Some(*r))?),
                _ => {}
            }
        }
        if master.as_deref() == Some("CARTESIAN")
            && let Some(pt) = pt
        {
            return Ok(Trim { par: None, pt: Some(pt) });
        }
        if let Some(par) = par {
            return Ok(Trim { par: Some(par), pt: None });
        }
        Ok(match pt {
            Some(pt) => Trim { par: None, pt: Some(pt) },
            None => Trim::default(),
        })
    };
    let t1 = pick(g, g.a(id, "Trim1")?)?;
    let t2 = pick(g, g.a(id, "Trim2")?)?;
    if bt == Some("IFCCIRCLE") || bt == Some("IFCELLIPSE") {
        let ang = |g: &mut GeoCtx<'_>, tr: Trim| -> GR<f64> {
            if let Some(p) = tr.par {
                return Ok(p * g.angle_unit);
            }
            match tr.pt {
                // paramOf of a conic is never null
                Some(pt) => Ok(param_of(g, basis, &pt)?.unwrap_or(f64::NAN)),
                None => Ok(0.0),
            }
        };
        let a0 = ang(g, t1)?;
        let mut a1 = ang(g, t2)?;
        let mut turns = 0u32;
        if sense {
            while a1 <= a0 + 1e-12 {
                a1 += TAU;
                turns = turns.saturating_add(1);
                if turns > MAX_TURNS {
                    return Err(GeoErr("a trimmed conic never closes".into()));
                }
            }
            return conic(g, basis, a0, a1, false);
        }
        while a1 >= a0 - 1e-12 {
            a1 -= TAU;
            turns = turns.saturating_add(1);
            if turns > MAX_TURNS {
                return Err(GeoErr("a trimmed conic never closes".into()));
            }
        }
        return conic(g, basis, a0, a1, false);
    }
    if bt == Some("IFCLINE") {
        let p = |g: &GeoCtx<'_>, tr: Trim| -> GR<V3> {
            match tr.pt {
                Some(pt) => Ok(pt),
                None => line_point(g, basis, tr.par.unwrap_or(0.0)),
            }
        };
        let a = p(g, t1)?;
        let b = p(g, t2)?;
        return Ok(if sense { vec![a, b] } else { vec![b, a] });
    }
    // another bounded curve: cut its points at the trimming points (parameters along it are not defined here)
    let mut pts = curve_points(g, basis, depth.saturating_add(1))?;
    if let (Some(p1), Some(p2)) = (t1.pt, t2.pt)
        && pts.len() > 1
    {
        let near = |q: &V3| {
            let mut best = 0usize;
            let mut bd = f64::INFINITY;
            for (k, p) in pts.iter().enumerate() {
                let d = len(&sub(p, q));
                if d < bd {
                    bd = d;
                    best = k;
                }
            }
            best
        };
        let i = near(&p1);
        let j = near(&p2);
        pts = if i <= j {
            pts.get(i..=j).map(<[V3]>::to_vec).unwrap_or_default()
        } else {
            let mut v = pts.get(j..=i).map(<[V3]>::to_vec).unwrap_or_default();
            v.reverse();
            v
        };
        if let Some(f) = pts.first_mut() {
            *f = p1;
        }
        if let Some(l) = pts.last_mut() {
            *l = p2;
        }
    } else if !pts.is_empty() {
        let name = bt.and_then(|t| g.s.pretty(t)).unwrap_or("curve").to_string();
        g.note(format!("A trimmed {name} is drawn untrimmed"));
    }
    if !sense {
        pts.reverse();
    }
    Ok(pts)
}

/// The most knots a B-spline may expand to (multiplicities are counts from the file).
const MAX_KNOTS: usize = 1_000_000;

/// IfcBSplineCurveWithKnots (rational too): de Boor at evenly spaced parameters per knot span.
fn bspline(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Vec<V3>> {
    let p = g.na(id, "Degree", 1.0)?;
    let mut ctrl = Vec::new();
    for c in refs(g.a(id, "ControlPointsList")?) {
        ctrl.push(g.point(Some(c))?);
    }
    let mult: Vec<f64> = list_or_empty(g.a(id, "KnotMultiplicities")?)?.iter().map(|v| numd(Some(v), 1.0)).collect();
    let kv: Vec<f64> = list_or_empty(g.a(id, "Knots")?)?.iter().map(|v| numd(Some(v), 0.0)).collect();
    let w: Option<Vec<f64>> = if g.t(id) == Some("IFCRATIONALBSPLINECURVEWITHKNOTS") {
        Some(list_or_empty(g.a(id, "WeightsData")?)?.iter().map(|v| numd(Some(v), 1.0)).collect())
    } else {
        None
    };
    let mut knots = Vec::new();
    for (i, &k) in kv.iter().enumerate() {
        let m = super::math::or(mult.get(i).copied().unwrap_or(f64::NAN), 1.0);
        // for (r = 0; r < m; r++)
        let reps = if m > 0.0 { m.ceil() } else { 0.0 };
        if reps + crate::util::usize_f64(knots.len()) > crate::util::usize_f64(MAX_KNOTS) {
            return Err(GeoErr("Invalid array length".into()));
        }
        for _ in 0..count(reps)? {
            knots.push(k);
        }
    }
    sample_bspline(p, &ctrl, &knots, w.as_deref(), 12)
}

/// The highest degree evaluated (de Boor is quadratic in it; real files stay below 10).
const MAX_DEGREE: f64 = 1000.0;

pub fn sample_bspline(p: f64, ctrl: &[V3], knots: &[f64], w: Option<&[f64]>, per_span: usize) -> GR<Vec<V3>> {
    let n = ctrl.len();
    let nf = crate::util::usize_f64(n);
    if n < 2 || crate::util::usize_f64(knots.len()) != nf + p + 1.0 {
        return Ok(ctrl.to_vec());
    }
    // degree 1 is the polyline through its control points (weights do not bend it)
    if p == 1.0 {
        return Ok(ctrl.to_vec());
    }
    // control points in order along one straight line: the curve is that segment (as its neighbours' plain lines draw it)
    if let (Some(a), Some(b)) = (ctrl.first(), ctrl.last()) {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l = hypot3(d[0], d[1], d[2]);
        if l > 0.0 {
            let mut straight = true;
            let mut last = -1e-12;
            for c in ctrl {
                let t = ((c[0] - a[0]) * d[0] + (c[1] - a[1]) * d[1] + (c[2] - a[2]) * d[2]) / (l * l);
                let off = hypot3(c[0] - a[0] - d[0] * t, c[1] - a[1] - d[1] * t, c[2] - a[2] - d[2] * t);
                if off > l * 1e-9 || t < last - 1e-12 {
                    straight = false;
                    break;
                }
                last = t;
            }
            if straight {
                return Ok(vec![*a, *b]);
            }
        }
    }
    // a negative degree reads d[p] of an empty list, a degree past the control points H[j] past its end (TypeErrors
    // in the JS)
    if p < 0.0 || p > MAX_DEGREE || p >= nf {
        return Err(super::ctx::type_error_read("undefined", "0"));
    }
    let pu = crate::util::f64_index(p, usize::MAX).unwrap_or(0);
    let h: Vec<[f64; 4]> = ctrl
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let wi = match w {
                Some(w) => w.get(i).copied().unwrap_or(1.0),
                None => 1.0,
            };
            [c[0] * wi, c[1] * wi, c[2] * wi, wi]
        })
        .collect();
    let kn = |i: usize| knots.get(i).copied().unwrap_or(f64::NAN);
    let lo = kn(pu);
    let hi = kn(n);
    let eval_at = |u: f64| -> V3 {
        let mut k = pu;
        while k < n.saturating_sub(1) && u >= kn(k.saturating_add(1)) {
            k = k.saturating_add(1);
        }
        let base = k.saturating_sub(pu);
        let mut d: Vec<[f64; 4]> = (0..=pu).map(|j| h.get(base.saturating_add(j)).copied().unwrap_or([f64::NAN; 4])).collect();
        for r in 1..=pu {
            for j in (r..=pu).rev() {
                let i = base.saturating_add(j);
                let den = kn(i.saturating_add(pu).saturating_add(1).saturating_sub(r)) - kn(i);
                let a = if super::math::falsy(den) { 0.0 } else { (u - kn(i)) / den };
                let prev = d.get(j.saturating_sub(1)).copied().unwrap_or([f64::NAN; 4]);
                if let Some(dj) = d.get_mut(j) {
                    for (x, p) in dj.iter_mut().zip(prev.iter()) {
                        *x = (1.0 - a) * p + a * *x;
                    }
                }
            }
        }
        let hh = d.get(pu).copied().unwrap_or([f64::NAN; 4]);
        [hh[0] / hh[3], hh[1] / hh[3], hh[2] / hh[3]]
    };
    let mut out = Vec::new();
    let psf = crate::util::usize_f64(per_span);
    for k in pu..n {
        let (a, b) = (kn(k), kn(k.saturating_add(1)));
        if b > a {
            for s in 0..per_span {
                out.push(eval_at(a + (b - a) * crate::util::usize_f64(s) / psf));
            }
        }
    }
    out.push(eval_at(hi - (hi - lo) * 1e-12));
    if let Some(hl) = h.get(n.saturating_sub(1))
        && !super::math::falsy(hl[3])
        && let Some(l) = out.last_mut()
    {
        *l = [hl[0] / hl[3], hl[1] / hl[3], hl[2] / hl[3]];
    }
    Ok(out)
}

fn offset_curve_2d(g: &mut GeoCtx<'_>, id: Option<Id>, depth: u32) -> GR<Vec<V3>> {
    let bc = g.ra(id, "BasisCurve")?;
    let pts = curve_points(g, bc, depth.saturating_add(1))?;
    let d = g.na(id, "Distance", 0.0)?;
    Ok(offset_polyline(&pts, d))
}

/// Offset of a 2D polyline to the left by d (mitred corners).
pub fn offset_polyline(pts: &[V3], d: f64) -> Vec<V3> {
    if pts.len() < 2 || super::math::falsy(d) {
        return pts.to_vec();
    }
    let nrm = |a: &V3, b: &V3| {
        let l = super::math::or(hypot2(b[0] - a[0], b[1] - a[1]), 1.0);
        [-(b[1] - a[1]) / l, (b[0] - a[0]) / l]
    };
    let mut out = Vec::with_capacity(pts.len());
    let n = pts.len();
    for (i, p) in pts.iter().enumerate() {
        let n0 = if i > 0 { pts.get(i.saturating_sub(1)).map(|q| nrm(q, p)) } else { None };
        let n1 = if i.saturating_add(1) < n { pts.get(i.saturating_add(1)).map(|q| nrm(p, q)) } else { None };
        let v = match (n0, n1) {
            (Some(a), Some(b)) => [a[0] + b[0], a[1] + b[1]],
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => [f64::NAN, f64::NAN],
        };
        let l = super::math::or(hypot2(v[0], v[1]), 1.0);
        let v = [v[0] / l, v[1] / l];
        let c = match (n0, n1) {
            (Some(_), Some(b)) => max(0.2, v[0] * b[0] + v[1] * b[1]),
            _ => 1.0,
        };
        out.push([p[0] + v[0] * d / c, p[1] + v[1] * d / c, super::math::or(p[2], 0.0)]);
    }
    out
}
