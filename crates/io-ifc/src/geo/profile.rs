//! geo-profile.js — IFC profiles as 2D outlines (IFC4 ADD2 TC1 §8.15 Profile resource). Parameterized shapes are drawn
//! in their own definition (dimensions, fillet and edge radii as true arcs, flange and leg slopes), centred as the
//! standard places them (bounding-box centre at the origin), then moved by the profile's Position.
//!
//! `profile_shapes(g, id)` → [{ outer (counter-clockwise), holes (clockwise), open }]; [] when nothing can be drawn.

use super::ctx::{GR, GeoCtx, refs};
use super::curve::{arc_segments, count, curve_points, offset_polyline};
use super::math::{M4, V2, V3, apply, area2};
use crate::jsmath::{JsMath, hypot2, max, min};
use crate::model::Id;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub outer: Vec<V2>,
    pub holes: Vec<Vec<V2>>,
    pub open: bool,
}

fn sh(outer: Vec<V2>) -> Vec<Shape> {
    vec![Shape { outer, holes: Vec::new(), open: false }]
}

pub fn profile_shapes(g: &mut GeoCtx<'_>, id: Option<Id>, depth: u32) -> GR<Vec<Shape>> {
    if id.is_none() || depth > 16 {
        return Ok(Vec::new());
    }
    let t = g.t(id).unwrap_or("");
    let n = |g: &GeoCtx<'_>, name: &str| g.na(id, name, 0.0);
    let shapes: Vec<Shape> = match t {
        "IFCRECTANGLEPROFILEDEF" => {
            let x = n(g, "XDim")? / 2.0;
            let y = n(g, "YDim")? / 2.0;
            sh(vec![[-x, -y], [x, -y], [x, y], [-x, y]])
        }
        "IFCROUNDEDRECTANGLEPROFILEDEF" => {
            let x = n(g, "XDim")? / 2.0;
            let y = n(g, "YDim")? / 2.0;
            let r = min(min(n(g, "RoundingRadius")?, x), y);
            sh(rounded(&[[-x, -y], [x, -y], [x, y], [-x, y]], &[r, r, r, r])?)
        }
        "IFCRECTANGLEHOLLOWPROFILEDEF" => {
            let x = n(g, "XDim")? / 2.0;
            let y = n(g, "YDim")? / 2.0;
            let w = n(g, "WallThickness")?;
            let ro = min(min(n(g, "OuterFilletRadius")?, x), y);
            let ri = min(min(n(g, "InnerFilletRadius")?, x - w), y - w);
            let (xi, yi) = (x - w, y - w);
            let holes = if xi > 0.0 && yi > 0.0 {
                let mut h = rounded(&[[-xi, -yi], [xi, -yi], [xi, yi], [-xi, yi]], &[ri, ri, ri, ri])?;
                h.reverse();
                vec![h]
            } else {
                Vec::new()
            };
            vec![Shape { outer: rounded(&[[-x, -y], [x, -y], [x, y], [-x, y]], &[ro, ro, ro, ro])?, holes, open: false }]
        }
        "IFCCIRCLEPROFILEDEF" => sh(circle(n(g, "Radius")?, None)?),
        "IFCCIRCLEHOLLOWPROFILEDEF" => {
            let r = n(g, "Radius")?;
            let w = n(g, "WallThickness")?;
            let holes = if r - w > 0.0 {
                let mut h = circle(r - w, None)?;
                h.reverse();
                vec![h]
            } else {
                Vec::new()
            };
            vec![Shape { outer: circle(r, None)?, holes, open: false }]
        }
        "IFCELLIPSEPROFILEDEF" => sh(circle(n(g, "SemiAxis1")?, Some(n(g, "SemiAxis2")?))?),
        "IFCISHAPEPROFILEDEF" | "IFCASYMMETRICISHAPEPROFILEDEF" => sh(i_shape(g, id, t)?),
        "IFCLSHAPEPROFILEDEF" => sh(l_shape(g, id)?),
        "IFCTSHAPEPROFILEDEF" => sh(t_shape(g, id)?),
        "IFCUSHAPEPROFILEDEF" => sh(u_shape(g, id)?),
        "IFCCSHAPEPROFILEDEF" => sh(c_shape(g, id)?),
        "IFCZSHAPEPROFILEDEF" => sh(z_shape(g, id)?),
        "IFCTRAPEZIUMPROFILEDEF" => {
            let b = n(g, "BottomXDim")?;
            let tx = n(g, "TopXDim")?;
            let y = n(g, "YDim")? / 2.0;
            let off = n(g, "TopXOffset")?;
            sh(vec![[-b / 2.0, -y], [b / 2.0, -y], [-b / 2.0 + off + tx, y], [-b / 2.0 + off, y]])
        }
        "IFCARBITRARYCLOSEDPROFILEDEF" | "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
            let oc = g.ra(id, "OuterCurve")?;
            let outer = flat(&curve_points(g, oc, 0)?);
            let mut holes = Vec::new();
            for c in refs(g.a(id, "InnerCurves")?) {
                let h = flat(&curve_points(g, Some(c), 0)?);
                if h.len() >= 3 {
                    holes.push(h);
                }
            }
            let s = if outer.len() >= 3 { vec![Shape { outer, holes, open: false }] } else { Vec::new() };
            if s.is_empty() {
                g.note("A closed profile has no usable outline");
            }
            s
        }
        "IFCARBITRARYOPENPROFILEDEF" | "IFCCENTERLINEPROFILEDEF" => {
            let c = g.ra(id, "Curve")?;
            let pts = flat(&curve_points(g, c, 0)?);
            let th = if t == "IFCCENTERLINEPROFILEDEF" { n(g, "Thickness")? } else { 0.0 };
            if th > 0.0 && pts.len() >= 2 {
                let p3: Vec<V3> = pts.iter().map(|p| [p[0], p[1], 0.0]).collect();
                let mut outer: Vec<V2> = offset_polyline(&p3, th / 2.0).iter().map(|p| [p[0], p[1]]).collect();
                let mut back: Vec<V2> = offset_polyline(&p3, -th / 2.0).iter().map(|p| [p[0], p[1]]).collect();
                back.reverse();
                outer.extend(back);
                sh(outer)
            } else if pts.len() >= 2 {
                vec![Shape { outer: pts, holes: Vec::new(), open: true }]
            } else {
                Vec::new()
            }
        }
        "IFCCOMPOSITEPROFILEDEF" => {
            let mut out = Vec::new();
            for p in refs(g.a(id, "Profiles")?) {
                out.extend(profile_shapes(g, Some(p), depth.saturating_add(1))?);
            }
            out
        }
        "IFCDERIVEDPROFILEDEF" | "IFCMIRROREDPROFILEDEF" => {
            let pp = g.ra(id, "ParentProfile")?;
            let parent = profile_shapes(g, pp, depth.saturating_add(1))?;
            let mut m = g.operator(g.ra(id, "Operator")?)?;
            if t == "IFCMIRROREDPROFILEDEF" {
                m = [-1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
            }
            parent.iter().map(|s| map_shape(s, &m)).collect()
        }
        other => {
            if !other.is_empty() {
                let p = g.pretty(other);
                g.note(format!("{p} profiles are not drawn yet"));
            }
            return Ok(Vec::new());
        }
    };
    let pos = g.ra(id, "Position")?;
    let shapes = if pos.is_some() && t != "IFCDERIVEDPROFILEDEF" && t != "IFCMIRROREDPROFILEDEF" {
        let mut out = Vec::with_capacity(shapes.len());
        for s in &shapes {
            let m = g.axis2(pos)?;
            out.push(map_shape(s, &m));
        }
        out
    } else {
        shapes
    };
    Ok(shapes.into_iter().map(orient).collect())
}

fn flat(pts: &[V3]) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::new();
    for p in pts {
        if out.last().is_none_or(|l| (l[0] - p[0]).abs() > 1e-12 || (l[1] - p[1]).abs() > 1e-12) {
            out.push([p[0], p[1]]);
        }
    }
    if out.len() > 2
        && let (Some(a), Some(b)) = (out.first(), out.last())
        && (a[0] - b[0]).abs() < 1e-9
        && (a[1] - b[1]).abs() < 1e-9
    {
        out.pop();
    }
    out
}

fn map_shape(s: &Shape, m: &M4) -> Shape {
    let f = |r: &Vec<V2>| -> Vec<V2> {
        r.iter()
            .map(|p| {
                let q = apply(m, p[0], p[1], 0.0);
                [q[0], q[1]]
            })
            .collect()
    };
    Shape { outer: f(&s.outer), holes: s.holes.iter().map(f).collect(), open: s.open }
}

/// Outer counter-clockwise, holes clockwise (a mirror flips them).
fn orient(mut s: Shape) -> Shape {
    if s.open {
        return s;
    }
    if area2(&s.outer) < 0.0 {
        s.outer.reverse();
    }
    for h in &mut s.holes {
        if area2(h) > 0.0 {
            h.reverse();
        }
    }
    s
}

fn circle(r1: f64, r2: Option<f64>) -> GR<Vec<V2>> {
    let r2 = r2.unwrap_or(r1);
    let tau = 2.0 * std::f64::consts::PI;
    let n = arc_segments(tau, max(r1, r2));
    let cnt = count(n)?;
    let mut out = Vec::with_capacity(cnt);
    for i in 0..cnt {
        let a = tau * crate::util::usize_f64(i) / n;
        out.push([r1 * a.jcos(), r2 * a.jsin()]);
    }
    Ok(out)
}

fn unit(v: V2) -> V2 {
    let l = super::math::or(hypot2(v[0], v[1]), 1.0);
    [v[0] / l, v[1] / l]
}

/// Replace each corner i of a closed polygon with an arc of radius r[i] tangent to both edges (0 = sharp).
pub fn rounded(pts: &[V2], r: &[f64]) -> GR<Vec<V2>> {
    let mut out = Vec::new();
    let n = pts.len();
    let pi = std::f64::consts::PI;
    for (i, &p) in pts.iter().enumerate() {
        let rad = super::math::or(r.get(i).copied().unwrap_or(0.0), 0.0);
        if rad <= 0.0 {
            out.push(p);
            continue;
        }
        let a = pts.get(if i == 0 { n.saturating_sub(1) } else { i.saturating_sub(1) }).copied().unwrap_or(p);
        let b = pts.get(super::math::next(i, n)).copied().unwrap_or(p);
        let u = unit([a[0] - p[0], a[1] - p[1]]);
        let v = unit([b[0] - p[0], b[1] - p[1]]);
        let cos_t = u[0] * v[0] + u[1] * v[1];
        let half = max(-1.0, min(1.0, cos_t)).jacos() / 2.0;
        if half < 1e-6 || (half - pi / 2.0).abs() < 1e-9 {
            out.push(p);
            continue;
        }
        let d = rad / half.jtan();
        let la = hypot2(a[0] - p[0], a[1] - p[1]);
        let lb = hypot2(b[0] - p[0], b[1] - p[1]);
        let dd = min(min(d, la * 0.999), lb * 0.999);
        let rr = dd * half.jtan();
        let s = [p[0] + u[0] * dd, p[1] + u[1] * dd];
        let e = [p[0] + v[0] * dd, p[1] + v[1] * dd];
        let bis = unit([u[0] + v[0], u[1] + v[1]]);
        let c = [p[0] + bis[0] * rr / half.jsin(), p[1] + bis[1] * rr / half.jsin()];
        let a0 = (s[1] - c[1]).jatan2(s[0] - c[0]);
        let a1 = (e[1] - c[1]).jatan2(e[0] - c[0]);
        let mut sweep = a1 - a0;
        while sweep > pi {
            sweep -= 2.0 * pi;
        }
        while sweep < -pi {
            sweep += 2.0 * pi;
        }
        let k = max(2.0, arc_segments(sweep.abs(), rr));
        let cnt = count(k + 1.0)?;
        for j in 0..cnt {
            let t = a0 + sweep * crate::util::usize_f64(j) / k;
            out.push([c[0] + rr * t.jcos(), c[1] + rr * t.jsin()]);
        }
    }
    Ok(out)
}

/// How far a face sloping at `slope` (radians) rises over a distance.
fn slope_drop(slope: f64, dist: f64) -> f64 {
    if super::math::falsy(slope) { 0.0 } else { slope.jtan() * dist }
}

fn i_shape(g: &GeoCtx<'_>, id: Option<Id>, t: &str) -> GR<Vec<V2>> {
    let n = |name: &str, d: f64| g.na(id, name, d);
    let asym = t == "IFCASYMMETRICISHAPEPROFILEDEF";
    let h = n("OverallDepth", 0.0)?;
    let tw = n("WebThickness", 0.0)?;
    let bw = if asym { n("BottomFlangeWidth", n("OverallWidth", 0.0)?)? } else { n("OverallWidth", 0.0)? };
    let bt = if asym { n("BottomFlangeThickness", n("FlangeThickness", 0.0)?)? } else { n("FlangeThickness", 0.0)? };
    let t_wid = if asym { n("TopFlangeWidth", bw)? } else { bw };
    let tt = if asym { n("TopFlangeThickness", bt)? } else { bt };
    let rb = if asym { n("BottomFlangeFilletRadius", n("FilletRadius", 0.0)?)? } else { n("FilletRadius", 0.0)? };
    let rt = if asym { n("TopFlangeFilletRadius", rb)? } else { rb };
    let eb = if asym { n("BottomFlangeEdgeRadius", n("FlangeEdgeRadius", 0.0)?)? } else { n("FlangeEdgeRadius", 0.0)? };
    let et = if asym { n("TopFlangeEdgeRadius", eb)? } else { eb };
    let sb = (if asym { n("BottomFlangeSlope", n("FlangeSlope", 0.0)?)? } else { n("FlangeSlope", 0.0)? }) * g.angle_unit;
    let st = (if asym { n("TopFlangeSlope", sb / g.angle_unit)? } else { n("FlangeSlope", 0.0)? }) * g.angle_unit;
    let (y0, y1, w, bb, tb) = (-h / 2.0, h / 2.0, tw / 2.0, bw / 2.0, t_wid / 2.0);
    // flange thickness is measured at the quarter point of the flange's outstand when it slopes (as rolled sections)
    let b_in = |x: f64| y0 + bt + slope_drop(sb, (bb - w) / 2.0 - (x - w));
    let t_in = |x: f64| y1 - tt - slope_drop(st, (tb - w) / 2.0 - (x - w));
    let pts = [
        [-bb, y0],
        [bb, y0],
        [bb, b_in(bb)],
        [w, b_in(w)],
        [w, t_in(w)],
        [tb, t_in(tb)],
        [tb, y1],
        [-tb, y1],
        [-tb, t_in(tb)],
        [-w, t_in(w)],
        [-w, b_in(w)],
        [-bb, b_in(bb)],
    ];
    rounded(&pts, &[0.0, 0.0, eb, rb, rt, et, 0.0, 0.0, et, rt, rb, eb])
}

fn l_shape(g: &GeoCtx<'_>, id: Option<Id>) -> GR<Vec<V2>> {
    let n = |name: &str, d: f64| g.na(id, name, d);
    let d = n("Depth", 0.0)?;
    let w = n("Width", d)?;
    let t = n("Thickness", 0.0)?;
    let r = n("FilletRadius", 0.0)?;
    let e = n("EdgeRadius", 0.0)?;
    let slope = n("LegSlope", 0.0)? * g.angle_unit;
    let (x0, y0) = (-w / 2.0, -d / 2.0);
    let mut pts = [[x0, y0], [x0 + w, y0], [x0 + w, y0 + t], [x0 + t, y0 + t], [x0 + t, y0 + d], [x0, y0 + d]];
    // a leg slope thickens the legs toward the heel; the thickness is held at the tips
    if !super::math::falsy(slope) {
        pts[3] = [x0 + t + slope_drop(slope, d - t), y0 + t + slope_drop(slope, w - t)];
    }
    rounded(&pts, &[0.0, 0.0, e, r, e, 0.0])
}

fn t_shape(g: &GeoCtx<'_>, id: Option<Id>) -> GR<Vec<V2>> {
    let n = |name: &str| g.na(id, name, 0.0);
    let (d, b, tw, tf) = (n("Depth")?, n("FlangeWidth")?, n("WebThickness")?, n("FlangeThickness")?);
    let (r, ef, ew) = (n("FilletRadius")?, n("FlangeEdgeRadius")?, n("WebEdgeRadius")?);
    let (y1, y0, bb, w) = (d / 2.0, -d / 2.0, b / 2.0, tw / 2.0);
    let pts = [[-w, y0], [w, y0], [w, y1 - tf], [bb, y1 - tf], [bb, y1], [-bb, y1], [-bb, y1 - tf], [-w, y1 - tf]];
    rounded(&pts, &[ew, ew, r, ef, 0.0, 0.0, ef, r])
}

fn u_shape(g: &GeoCtx<'_>, id: Option<Id>) -> GR<Vec<V2>> {
    let n = |name: &str| g.na(id, name, 0.0);
    let (d, b, tw, tf) = (n("Depth")?, n("FlangeWidth")?, n("WebThickness")?, n("FlangeThickness")?);
    let (r, e) = (n("FilletRadius")?, n("EdgeRadius")?);
    let slope = n("FlangeSlope")? * g.angle_unit;
    let (x0, y0, y1) = (-b / 2.0, -d / 2.0, d / 2.0);
    let drop = slope_drop(slope, (b - tw) / 2.0);
    let pts = [
        [x0, y0],
        [x0 + b, y0],
        [x0 + b, y0 + tf - drop],
        [x0 + tw, y0 + tf + drop],
        [x0 + tw, y1 - tf - drop],
        [x0 + b, y1 - tf + drop],
        [x0 + b, y1],
        [x0, y1],
    ];
    rounded(&pts, &[0.0, 0.0, e, r, r, e, 0.0, 0.0])
}

fn c_shape(g: &GeoCtx<'_>, id: Option<Id>) -> GR<Vec<V2>> {
    let n = |name: &str| g.na(id, name, 0.0);
    let (d, b, t, lip, r) = (n("Depth")?, n("Width")?, n("WallThickness")?, n("Girth")?, n("InternalFilletRadius")?);
    let (x0, y0, x1, y1) = (-b / 2.0, -d / 2.0, b / 2.0, d / 2.0);
    let ro = if super::math::falsy(r) { 0.0 } else { r + t };
    if lip > t {
        let pts = [
            [x0, y0],
            [x1, y0],
            [x1, y0 + lip],
            [x1 - t, y0 + lip],
            [x1 - t, y0 + t],
            [x0 + t, y0 + t],
            [x0 + t, y1 - t],
            [x1 - t, y1 - t],
            [x1 - t, y1 - lip],
            [x1, y1 - lip],
            [x1, y1],
            [x0, y1],
        ];
        rounded(&pts, &[ro, ro, 0.0, 0.0, r, r, r, r, 0.0, 0.0, ro, ro])
    } else {
        let pts = [[x0, y0], [x1, y0], [x1, y0 + t], [x0 + t, y0 + t], [x0 + t, y1 - t], [x1, y1 - t], [x1, y1], [x0, y1]];
        rounded(&pts, &[ro, 0.0, 0.0, r, r, 0.0, 0.0, ro])
    }
}

fn z_shape(g: &GeoCtx<'_>, id: Option<Id>) -> GR<Vec<V2>> {
    let n = |name: &str| g.na(id, name, 0.0);
    let (d, b, tw, tf) = (n("Depth")?, n("FlangeWidth")?, n("WebThickness")?, n("FlangeThickness")?);
    let (r, e) = (n("FilletRadius")?, n("EdgeRadius")?);
    let (w, y0, y1) = (tw / 2.0, -d / 2.0, d / 2.0);
    let pts = [[-w, y0], [b - w, y0], [b - w, y0 + tf], [w, y0 + tf], [w, y1], [-b + w, y1], [-b + w, y1 - tf], [-w, y1 - tf]];
    rounded(&pts, &[0.0, 0.0, e, r, 0.0, 0.0, e, r])
}
