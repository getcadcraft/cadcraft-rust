//! Site plans: io.js `importDXF` (group-code reading adapted from CadCraft's dxf.js; only what a site plan needs) and
//! `importDWG` (cc-dwg's drawing model, as JSON — this crate may not use cc-io-dwg, see the crate docs). Both are
//! first read into one plain shape, entities + blocks, and `siteEdges` turns that into edges on the ground: arcs and
//! circles as 48 segments per turn, splines 12 per knot span, blocks expanded (16 deep), text in strokefont's
//! single-stroke font; hatches become faces on the ground (with their holes) and 3D faces keep their heights.
//!
//! Hostile input: at most 2 million line pieces, 400 000 text pieces, 200 000 faces; coordinates within 10 000 km.
//! Where the JS would loop for ever or run out of memory on a crafted file (an angle sweep of 1e300 radians, a block
//! array of a million copies nested 16 deep, a NURBS of degree 100 000) this port stops with a plain error instead
//! (Spec gap: the JS never answers there).

use std::f64::consts::PI;

use cc_m3d::Model;
use cc_m3d::js;
use cc_m3d::meshin::{PolyIn, edges_def_in, mesh_def_in};
use cc_m3d::vec::P3;
use serde_json::{Map, Value};

use crate::Res;
use crate::i18n::{err, t};
use crate::jsx::{arr, atan, atan2, cos, fin, get, hypot, sin, truthy};
use crate::num::{is_space, parse_float, parse_int, to_i32};
use crate::obj::Imported;
use crate::strokefont::{clean_dxf_text, clean_mtext, text_strokes, text_width};
use crate::util::{clean_name, skip};

type P2 = [f64; 2];

const CIRCLE_SEGS: f64 = 48.0;
const SPLINE_PER_SPAN: f64 = 12.0;
const MAX_SEGS: usize = 2_000_000;
const MAX_TEXT_SEGS: f64 = 400_000.0;
/// Most entity visits a drawing may cost (block arrays nested in blocks multiply; the JS never finishes there).
const MAX_DRAW: u64 = 100_000_000;
/// the import refuses a drawing with more filled outlines than this (site.js checks after drawing; nested inserts can
/// make far more first, so the drawing stops as soon as the count is passed: same refusal, bounded memory)
const MAX_POLYS: usize = 200_000;
/// Most points one arc or curve may be cut into before it is refused (the JS runs out of memory there).
const MAX_ARC_POINTS: f64 = 50_000_000.0;

/// A text entity (TEXT, MTEXT, visible ATTRIB).
#[derive(Clone, Debug)]
pub struct Text {
    pub lines: Vec<String>,
    pub h: f64,
    pub wf: f64,
    pub oblique: f64,
    pub rot: f64,
    pub x: f64,
    pub y: f64,
    /// MTEXT's attachment point (1–9); None for TEXT.
    pub attach: Option<f64>,
    pub h_align: f64,
    pub v_align: f64,
    pub ax: f64,
    pub ay: f64,
    /// MTEXT's reference width (NaN: none).
    pub width: f64,
    pub spacing: f64,
    pub mirror: bool,
}

/// A hatch edge, as the DXF and DWG hatch readers hand them to `loopPoints`.
#[derive(Clone, Debug)]
pub enum HEdge {
    Line(P2, P2),
    Arc { c: P2, r: f64, a0: f64, a1: f64, ccw: bool },
    Ell { c: P2, maj: Option<P2>, ratio: f64, a0: f64, a1: f64, ccw: bool },
    Spline { degree: f64, knots: Knots, ctrl: Vec<P2>, weights: Option<Vec<f64>> },
}

/// Knot values as the JS sees them: a list of numbers (maybe not all finite), or something that is not one.
#[derive(Clone, Debug)]
pub enum Knots {
    List(Vec<Option<f64>>),
    /// not an array: its `length` (a string has one; anything else is undefined)
    Other(Option<usize>),
}

/// One entity of the plain shape both readers make.
#[derive(Clone, Debug)]
pub enum Ent {
    Paper,
    Nothing,
    Skip(String),
    Line(P2, P2),
    Poly { vs: Vec<(P2, f64)>, closed: bool },
    Circle { c: P2, r: f64 },
    Arc { c: P2, r: f64, a0: f64, a1: f64 },
    Insert { block: String, x: f64, y: f64, sx: f64, sy: f64, rot: f64, cols: f64, rows: f64, dc: f64, dr: f64, attribs: Vec<Text> },
    Curve { spline: bool, pts: Vec<P2>, closed: bool },
    Face(Vec<P3>),
    Hatch(Vec<Vec<P2>>),
    Text(Text),
    Dim(String),
}

/// A block: base point and entities.
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub base: P2,
    pub ents: Vec<Ent>,
}

/// The plain shape: $INSUNITS, model-space entities, blocks by upper-case name.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub ins_units: f64,
    pub ents: Vec<Ent>,
    pub blocks: std::collections::HashMap<String, Block>,
}

// ---------------------------------------------------------------------------------------------------------------
// curves shared by the DXF and DWG paths (plain 2D points, drawing units)

/// `while (sw <= 0) sw += 2π` (repeated additions, as the JS rounds them); None where it would never end.
fn wrap_sweep(mut sw: f64) -> Option<f64> {
    let mut guard = 0u64;
    while sw <= 0.0 {
        sw += 2.0 * PI;
        guard = guard.saturating_add(1);
        if guard > 100_000_000 {
            return None;
        }
    }
    Some(sw)
}

/// `ellipseCurve(c, maj, ratio, t0, t1, flip)`.
fn ellipse_curve(c: P2, maj: P2, ratio: f64, t0: f64, t1: f64, flip: bool) -> Res<Ent> {
    if !(hypot(&[maj[0], maj[1]]) > 0.0) || !(ratio > 0.0) {
        return Ok(Ent::Nothing);
    }
    let mn = if flip { [maj[1] * ratio, -maj[0] * ratio] } else { [-maj[1] * ratio, maj[0] * ratio] };
    let sw = wrap_sweep(t1 - t0).ok_or_else(|| err("m3d.io.this_drawing_has_more_than_2"))?;
    let full = (sw - 2.0 * PI).abs() < 1e-9;
    let n = (4.0f64).max((sw / (2.0 * PI / CIRCLE_SEGS)).ceil());
    if n > MAX_ARC_POINTS {
        return Err(err("m3d.io.this_drawing_has_more_than_2"));
    }
    let mut pts = Vec::new();
    let last = if full { n - 1.0 } else { n };
    let mut k = 0.0;
    while k <= last {
        let tt = t0 + (sw * k) / n;
        pts.push([c[0] + maj[0] * cos(tt) + mn[0] * sin(tt), c[1] + maj[1] * cos(tt) + mn[1] * sin(tt)]);
        k += 1.0;
    }
    Ok(Ent::Curve { spline: false, pts, closed: full })
}

/// `splineCurve(degree, knots, ctrl, weights, fit, closed)`.
fn spline_curve(degree: f64, knots: &Knots, ctrl: &[P2], weights: Option<&[f64]>, fit: &[P2], closed: bool) -> Res<Ent> {
    if let Some(pts) = nurbs_pts(degree, knots, ctrl, weights)? {
        return Ok(Ent::Curve { spline: true, pts, closed: false });
    }
    if fit.len() >= 2 {
        return Ok(Ent::Curve { spline: true, pts: fit_curve(fit, closed), closed });
    }
    Ok(Ent::Skip(t("m3d.io.spline_no_points", &[])))
}

/// `fitCurve(F, closed)`: centripetal Catmull–Rom through fit points, 12 points per span.
fn fit_curve(f: &[P2], closed: bool) -> Vec<P2> {
    let n = f.len();
    if n < 3 {
        return f.to_vec();
    }
    let g = |i: usize| f.get(i).copied().unwrap_or([f64::NAN; 2]);
    let ni = i64::try_from(n).unwrap_or(i64::MAX);
    let at = |i: i64| -> P2 {
        if closed {
            let k = usize::try_from(i.saturating_add(ni).rem_euclid(ni.max(1))).unwrap_or(0);
            return g(k);
        }
        if i < 0 {
            return [2.0 * g(0)[0] - g(1)[0], 2.0 * g(0)[1] - g(1)[1]];
        }
        if i >= ni {
            let (a, b) = (g(n.saturating_sub(1)), g(n.saturating_sub(2)));
            return [2.0 * a[0] - b[0], 2.0 * a[1] - b[1]];
        }
        g(usize::try_from(i).unwrap_or(0))
    };
    let mut out = Vec::new();
    let spans = if closed { ni } else { ni.saturating_sub(1) };
    let tj = |a: P2, b: P2| (1e-12f64).max(hypot(&[b[0] - a[0], b[1] - a[1]]).sqrt());
    for i in 0..spans {
        let (p0, p1, p2, p3) = (at(i.saturating_sub(1)), at(i), at(i.saturating_add(1)), at(i.saturating_add(2)));
        let t1 = tj(p0, p1);
        let t2 = t1 + tj(p1, p2);
        let t3 = t2 + tj(p2, p3);
        let mut k = 0.0;
        while k < SPLINE_PER_SPAN {
            let tt = t1 + ((t2 - t1) * k) / SPLINE_PER_SPAN;
            let l = |a: P2, b: P2, ta: f64, tb: f64| -> P2 {
                [((tb - tt) * a[0] + (tt - ta) * b[0]) / (tb - ta), ((tb - tt) * a[1] + (tt - ta) * b[1]) / (tb - ta)]
            };
            let a1 = l(p0, p1, 0.0, t1);
            let a2 = l(p1, p2, t1, t2);
            let a3 = l(p2, p3, t2, t3);
            let b1 = l(a1, a2, 0.0, t2);
            let b2 = l(a2, a3, t1, t3);
            out.push(l(b1, b2, t1, t2));
            k += 1.0;
        }
    }
    if !closed {
        out.push(g(n.saturating_sub(1)));
    }
    out
}

/// `nurbsPts(p, U, P, W)`: a NURBS curve (de Boor) sampled 12 points per knot span; None when it cannot be one.
fn nurbs_pts(p: f64, u: &Knots, cp: &[P2], w: Option<&[f64]>) -> Res<Option<Vec<P2>>> {
    let n = cp.len();
    let nf = n as f64;
    if !(p >= 1.0) || nf < p + 1.0 {
        return Ok(None);
    }
    let ulen = match u {
        Knots::List(l) => Some(l.len()),
        Knots::Other(l) => *l,
    };
    if ulen.map(|l| l as f64) != Some(nf + p + 1.0) {
        return Ok(None);
    }
    if cp.iter().any(|q| !q.iter().all(|x| x.is_finite())) {
        return Ok(None);
    }
    let uk: Vec<f64> = match u {
        Knots::List(l) => {
            if l.iter().any(|x| !x.is_some_and(f64::is_finite)) {
                return Ok(None);
            }
            l.iter().map(|x| x.unwrap_or(f64::NAN)).collect()
        }
        // a string of the right length: `U.every` is not a function
        Knots::Other(_) => return Err(cc_m3d::Error::Type),
    };
    // p is now a whole number with 1 ≤ p < n
    let pu = crate::num::index(p).unwrap_or(0);
    if pu == 0 || pu >= n {
        return Ok(None);
    }
    let ug = |i: usize| uk.get(i).copied().unwrap_or(f64::NAN);
    let a = ug(pu);
    let b = ug(n);
    if !(b > a) {
        return Ok(None);
    }
    // the JS gives up (null) on 200 000 points or more: count them first; and refuse what it would never finish
    let spans = (pu..n).filter(|&k| ug(k.saturating_add(1)) > ug(k)).count();
    let total = spans.saturating_mul(12).saturating_add(1);
    if total >= 200_000 {
        return Ok(None);
    }
    if (total as f64) * (p * p) > 2e9 {
        return Err(err("m3d.io.this_drawing_has_more_than_2"));
    }
    let wv = |i: usize| -> f64 {
        match w {
            None => 1.0,
            Some(ws) => match ws.get(i) {
                Some(&x) if x != 0.0 && !x.is_nan() => x,
                _ => 1.0,
            },
        }
    };
    let point = |uu: f64| -> P2 {
        let mut k = pu;
        while k < n.saturating_sub(1) && uu >= ug(k.saturating_add(1)) {
            k = k.saturating_add(1);
        }
        let base = k.saturating_sub(pu);
        let mut d: Vec<[f64; 3]> = Vec::with_capacity(pu.saturating_add(1));
        for j in 0..=pu {
            let q = cp.get(base.saturating_add(j)).copied().unwrap_or([f64::NAN; 2]);
            let ww = wv(base.saturating_add(j));
            d.push([q[0] * ww, q[1] * ww, ww]);
        }
        for r in 1..=pu {
            let mut j = pu;
            while j >= r {
                let i = base.saturating_add(j);
                let den = ug(i.saturating_add(pu).saturating_sub(r).saturating_add(1)) - ug(i);
                let al = if den > 0.0 { (uu - ug(i)) / den } else { 0.0 };
                let prev = d.get(j.saturating_sub(1)).copied().unwrap_or([f64::NAN; 3]);
                if let Some(dj) = d.get_mut(j) {
                    *dj = [(1.0 - al) * prev[0] + al * dj[0], (1.0 - al) * prev[1] + al * dj[1], (1.0 - al) * prev[2] + al * dj[2]];
                }
                if j == 0 {
                    break;
                }
                j = j.saturating_sub(1);
            }
        }
        let dp = d.get(pu).copied().unwrap_or([f64::NAN; 3]);
        let w = if dp[2] != 0.0 && !dp[2].is_nan() { dp[2] } else { 1.0 };
        [dp[0] / w, dp[1] / w]
    };
    let mut out = Vec::with_capacity(total);
    for k in pu..n {
        let (u0, u1) = (ug(k), ug(k.saturating_add(1)));
        if !(u1 > u0) {
            continue;
        }
        let mut j = 0.0;
        while j < SPLINE_PER_SPAN {
            out.push(point(u0 + ((u1 - u0) * j) / SPLINE_PER_SPAN));
            j += 1.0;
        }
    }
    out.push(point(b - (b - a) * 1e-12));
    Ok(if out.len() >= 2 && out.len() < 200_000 { Some(out) } else { None })
}

/// `faceEnt(P)`: four corners (the last may repeat the third) → a face, or nothing when degenerate.
fn face_ent(pts: Vec<P3>) -> Ent {
    let mut q: Vec<P3> = Vec::new();
    for x in pts {
        let keep = match q.last() {
            None => true,
            Some(l) => hypot(&[x[0] - l[0], x[1] - l[1], x[2] - l[2]]) > 1e-12,
        };
        if keep {
            q.push(x);
        }
    }
    while q.len() > 1 {
        let (f, l) = (q.first().copied().unwrap_or([0.0; 3]), q.last().copied().unwrap_or([0.0; 3]));
        if hypot(&[f[0] - l[0], f[1] - l[1], f[2] - l[2]]) <= 1e-12 {
            q.pop();
        } else {
            break;
        }
    }
    if q.len() >= 3 { Ent::Face(q) } else { Ent::Nothing }
}

/// A hatch loop: a polyline with bulges, or edges.
pub enum LoopIn<'a> {
    Poly(&'a [(Option<P2>, f64)]),
    Edges(&'a [HEdge]),
}

/// `loopPoints(l)`: a hatch loop → its points (closed, last ≠ first).
fn loop_points(l: LoopIn<'_>) -> Res<Vec<P2>> {
    let mut out: Vec<P2> = Vec::new();
    let put = |out: &mut Vec<P2>, q: P2| {
        let keep = match out.last() {
            None => true,
            Some(l) => hypot(&[q[0] - l[0], q[1] - l[1]]) > 1e-12,
        };
        if keep {
            out.push(q);
        }
    };
    let arc_run = |out: &mut Vec<P2>, c: P2, r: f64, a0: f64, a1: f64, ccw: bool, ell: Option<(P2, P2)>| -> Res<()> {
        let s0 = a0;
        let mut sw = wrap_sweep(a1 - s0).ok_or_else(|| err("m3d.io.this_drawing_has_more_than_2"))?;
        if (a1 - a0).abs() >= 2.0 * PI - 1e-9 {
            sw = 2.0 * PI;
        }
        let n = (2.0f64).max((sw / (2.0 * PI / CIRCLE_SEGS)).ceil());
        if n > MAX_ARC_POINTS {
            return Err(err("m3d.io.this_drawing_has_more_than_2"));
        }
        let mut k = 0.0;
        while k <= n {
            let mut tt = s0 + (sw * k) / n;
            if !ccw {
                tt = -tt;
            }
            match ell {
                Some((maj, mn)) => put(out, [c[0] + maj[0] * cos(tt) + mn[0] * sin(tt), c[1] + maj[1] * cos(tt) + mn[1] * sin(tt)]),
                None => put(out, [c[0] + r * cos(tt), c[1] + r * sin(tt)]),
            }
            k += 1.0;
        }
        Ok(())
    };
    match l {
        LoopIn::Poly(vs_in) => {
            let vs: Vec<(P2, f64)> =
                vs_in.iter().filter_map(|(p, b)| p.filter(|p| p.iter().all(|x| x.is_finite())).map(|p| (p, *b))).collect();
            let n = vs.len();
            for (j, (a, bu)) in vs.iter().enumerate() {
                let b = vs.get(j.saturating_add(1).checked_rem(n).unwrap_or(0)).map_or([f64::NAN; 2], |v| v.0);
                let bu = if *bu != 0.0 && !bu.is_nan() { *bu } else { 0.0 };
                if bu.abs() < 1e-12 {
                    put(&mut out, *a);
                    continue;
                }
                let sc = (1.0 / bu - bu) / 2.0;
                let c = [(a[0] + b[0]) / 2.0 - sc * (b[1] - a[1]) / 2.0, (a[1] + b[1]) / 2.0 + sc * (b[0] - a[0]) / 2.0];
                let r = hypot(&[a[0] - c[0], a[1] - c[1]]);
                let st = atan2(a[1] - c[1], a[0] - c[0]);
                let sw = 4.0 * atan(bu);
                let nn = (2.0f64).max((sw.abs() / (2.0 * PI / CIRCLE_SEGS)).ceil());
                let mut k = 0.0;
                while k < nn {
                    put(&mut out, [c[0] + r * cos(st + (sw * k) / nn), c[1] + r * sin(st + (sw * k) / nn)]);
                    k += 1.0;
                }
            }
        }
        LoopIn::Edges(edges) => {
            for d in edges {
                match d {
                    HEdge::Line(a, b) => {
                        put(&mut out, *a);
                        put(&mut out, *b);
                    }
                    HEdge::Arc { c, r, a0, a1, ccw } => {
                        if *r > 0.0 {
                            arc_run(&mut out, *c, *r, *a0, *a1, *ccw, None)?;
                        }
                    }
                    HEdge::Ell { c, maj, ratio, a0, a1, ccw } => {
                        if let Some(m) = maj
                            && *ratio > 0.0
                        {
                            arc_run(&mut out, *c, 0.0, *a0, *a1, *ccw, Some((*m, [-m[1] * ratio, m[0] * ratio])))?;
                        }
                    }
                    HEdge::Spline { degree, knots, ctrl, weights } => {
                        let pts = nurbs_pts(*degree, knots, ctrl, weights.as_deref())?;
                        for q in pts.as_deref().unwrap_or(ctrl) {
                            put(&mut out, *q);
                        }
                    }
                }
            }
        }
    }
    while out.len() > 1 {
        let (f, l) = (out.first().copied().unwrap_or([0.0; 2]), out.last().copied().unwrap_or([0.0; 2]));
        if hypot(&[f[0] - l[0], f[1] - l[1]]) <= 1e-9 {
            out.pop();
        } else {
            break;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------
// DXF

type Pair<'t> = (f64, &'t str);

/// `val(g, code)`: the first value of that code, trimmed.
fn val<'t>(g: &[Pair<'t>], code: f64) -> Option<&'t str> {
    g.iter().find(|(c, _)| *c == code).map(|(_, v)| js::trim(v))
}
/// `num(g, code, d)`.
fn num(g: &[Pair<'_>], code: f64, d: f64) -> f64 {
    match val(g, code) {
        None => d,
        Some(v) => {
            let x = parse_float(v);
            if x.is_finite() { x } else { d }
        }
    }
}
/// `parseInt(val(g, code) ?? dflt, 10)`.
fn pint(g: &[Pair<'_>], code: f64, dflt: &str) -> f64 {
    parse_int(val(g, code).unwrap_or(dflt))
}

/// `dxfText(g, f, vCode)`: TEXT / ATTRIB group codes → a text entity.
fn dxf_text(g: &[Pair<'_>], f: bool, v_code: f64) -> Ent {
    let value = clean_dxf_text(val(g, 1.0).unwrap_or(""));
    let h = num(g, 40.0, 0.0);
    if js::trim(&value).is_empty() || !(h > 0.0) {
        return Ent::Nothing;
    }
    let x = |v: f64| if f { -v } else { v };
    let or0 = |v: f64| if v.is_nan() || v == 0.0 { 0.0 } else { v };
    let h_align = or0(pint(g, 72.0, "0"));
    let v_align = or0(pint(g, v_code, "0"));
    let has_align = val(g, 11.0).is_some();
    let wf = num(g, 41.0, 1.0);
    Ent::Text(Text {
        lines: vec![value],
        h,
        wf: if wf != 0.0 { wf } else { 1.0 },
        oblique: num(g, 51.0, 0.0) * PI / 180.0,
        rot: num(g, 50.0, 0.0) * PI / 180.0,
        x: x(num(g, 10.0, 0.0)),
        y: num(g, 20.0, 0.0),
        attach: None,
        h_align,
        v_align,
        ax: if has_align { x(num(g, 11.0, 0.0)) } else { x(num(g, 10.0, 0.0)) },
        ay: if has_align { num(g, 21.0, 0.0) } else { num(g, 20.0, 0.0) },
        width: f64::NAN,
        spacing: 1.0,
        mirror: f,
    })
}

/// Cap a count read from the file (Spec gap: past this the JS runs out of memory pushing zeros).
fn cap_count(n: f64) -> f64 {
    n.min(4_000_000.0)
}

/// `dxfHatchLoops(g)`: HATCH boundary paths (group codes 91…97 as the DXF reference lays them out) → loops of points.
fn dxf_hatch_loops(g: &[Pair<'_>]) -> Res<Vec<Vec<P2>>> {
    let mut loops: Vec<Vec<P2>> = Vec::new();
    let Some(start) = g.iter().position(|(c, _)| *c == 91.0) else { return Ok(loops) };
    let n_loops = {
        let v = parse_int(g.get(start).map_or("", |p| p.1));
        if v.is_nan() || v == 0.0 { 0.0 } else { v }
    };
    let mut i = start.saturating_add(1);
    let take = |i: &mut usize, code: f64| -> Option<&str> {
        while *i < g.len() && g.get(*i).is_some_and(|p| p.0 != code) {
            *i = i.saturating_add(1);
        }
        let r = g.get(*i).map(|p| p.1);
        if r.is_some() {
            *i = i.saturating_add(1);
        }
        r
    };
    let n = |i: &mut usize, code: f64| -> f64 {
        let v = parse_float(take(i, code).unwrap_or("undefined"));
        if v.is_finite() { v } else { 0.0 }
    };
    let pi = |i: &mut usize, code: f64, d: &str| -> f64 { parse_int(take(i, code).unwrap_or(d)) };
    let mut l = 0.0;
    while l < n_loops && i < g.len() {
        let flags = pi(&mut i, 92.0, "0");
        if to_i32(flags) & 2 != 0 {
            let has_bulge = pi(&mut i, 72.0, "0");
            let has_bulge = has_bulge != 0.0 && !has_bulge.is_nan();
            let nv = cap_count(pi(&mut i, 93.0, "0"));
            let mut vs: Vec<(Option<P2>, f64)> = Vec::new();
            let mut k = 0.0;
            while k < nv && i < g.len() {
                let x = n(&mut i, 10.0);
                let y = n(&mut i, 20.0);
                let mut b = 0.0;
                if has_bulge && g.get(i).is_some_and(|p| p.0 == 42.0) {
                    let v = parse_float(g.get(i).map_or("", |p| p.1));
                    b = if v.is_nan() || v == 0.0 { 0.0 } else { v };
                    i = i.saturating_add(1);
                }
                vs.push((Some([x, y]), b));
                k += 1.0;
            }
            loops.push(loop_points(LoopIn::Poly(&vs))?);
        } else {
            let ne = cap_count(pi(&mut i, 93.0, "0"));
            let mut edges: Vec<HEdge> = Vec::new();
            let mut k = 0.0;
            while k < ne && i < g.len() {
                let et = pi(&mut i, 72.0, "0");
                if et == 1.0 {
                    let a = [n(&mut i, 10.0), n(&mut i, 20.0)];
                    let b = [n(&mut i, 11.0), n(&mut i, 21.0)];
                    edges.push(HEdge::Line(a, b));
                } else if et == 2.0 {
                    let c = [n(&mut i, 10.0), n(&mut i, 20.0)];
                    let r = n(&mut i, 40.0);
                    let a0 = n(&mut i, 50.0);
                    let a1 = n(&mut i, 51.0);
                    let ccw = pi(&mut i, 73.0, "1") != 0.0;
                    edges.push(HEdge::Arc { c, r, a0: a0 * PI / 180.0, a1: a1 * PI / 180.0, ccw });
                } else if et == 3.0 {
                    let c = [n(&mut i, 10.0), n(&mut i, 20.0)];
                    let maj = [n(&mut i, 11.0), n(&mut i, 21.0)];
                    let ratio = n(&mut i, 40.0);
                    let a0 = n(&mut i, 50.0);
                    let a1 = n(&mut i, 51.0);
                    let ccw = pi(&mut i, 73.0, "1") != 0.0;
                    edges.push(HEdge::Ell { c, maj: Some(maj), ratio, a0: a0 * PI / 180.0, a1: a1 * PI / 180.0, ccw });
                } else if et == 4.0 {
                    let degree = pi(&mut i, 94.0, "3");
                    let nk = cap_count(pi(&mut i, 95.0, "0"));
                    let nc = cap_count(pi(&mut i, 96.0, "0"));
                    let mut knots = Vec::new();
                    let mut q = 0.0;
                    while q < nk {
                        knots.push(Some(n(&mut i, 40.0)));
                        q += 1.0;
                    }
                    let mut ctrl = Vec::new();
                    let mut w = Vec::new();
                    let mut q = 0.0;
                    while q < nc {
                        ctrl.push([n(&mut i, 10.0), n(&mut i, 20.0)]);
                        if g.get(i).is_some_and(|p| p.0 == 42.0) {
                            let v = parse_float(g.get(i).map_or("", |p| p.1));
                            w.push(if v.is_nan() || v == 0.0 { 1.0 } else { v });
                            i = i.saturating_add(1);
                        }
                        q += 1.0;
                    }
                    let weights = if w.len() as f64 == nc { Some(w) } else { None };
                    edges.push(HEdge::Spline { degree, knots: Knots::List(knots), ctrl, weights });
                } else {
                    break;
                }
                k += 1.0;
            }
            loops.push(loop_points(LoopIn::Edges(&edges))?);
        }
        // source boundary objects (97 count, 330 handles) follow; take() skips them on the next loop
        l += 1.0;
    }
    loops.retain(|l| l.len() >= 3);
    Ok(loops)
}

/// `dxfEnt(type, g, follow, top)`.
#[allow(clippy::too_many_lines)]
fn dxf_ent(typ: &str, g: &[Pair<'_>], follow: Option<&[(String, Vec<Pair<'_>>)]>, top: bool) -> Res<Ent> {
    if top && val(g, 67.0) == Some("1") {
        return Ok(Ent::Paper);
    }
    if val(g, 60.0) == Some("1") {
        return Ok(Ent::Nothing);
    }
    let f = num(g, 230.0, 1.0) < 0.0;
    let x = |v: f64| if f { -v } else { v };
    Ok(match typ {
        "LINE" => Ent::Line([num(g, 10.0, 0.0), num(g, 20.0, 0.0)], [num(g, 11.0, 0.0), num(g, 21.0, 0.0)]),
        "LWPOLYLINE" => {
            let mut vs: Vec<(P2, f64)> = Vec::new();
            for (c, v) in g {
                if *c == 10.0 {
                    vs.push(([x(parse_float(v)), 0.0], 0.0));
                } else if *c == 20.0
                    && let Some(last) = vs.last_mut()
                {
                    last.0[1] = parse_float(v);
                } else if *c == 42.0
                    && let Some(last) = vs.last_mut()
                {
                    last.1 = (if f { -1.0 } else { 1.0 }) * parse_float(v);
                }
            }
            if vs.len() >= 2 && vs.iter().all(|(p, _)| p.iter().all(|x| x.is_finite())) {
                Ent::Poly { vs, closed: to_i32(pint(g, 70.0, "0")) & 1 == 1 }
            } else {
                Ent::Nothing
            }
        }
        "POLYLINE" => {
            let flags = pint(g, 70.0, "0");
            if to_i32(flags) & (16 | 64) != 0 {
                return Ok(Ent::Skip("POLYLINE (mesh)".into()));
            }
            let vs: Vec<(P2, f64)> = follow
                .unwrap_or(&[])
                .iter()
                .filter(|(ty, sg)| ty == "VERTEX" && to_i32(pint(sg, 70.0, "0")) & 16 == 0)
                .map(|(_, sg)| ([x(num(sg, 10.0, 0.0)), num(sg, 20.0, 0.0)], (if f { -1.0 } else { 1.0 }) * num(sg, 42.0, 0.0)))
                .collect();
            if vs.len() >= 2 { Ent::Poly { vs, closed: to_i32(flags) & 1 == 1 } } else { Ent::Nothing }
        }
        "CIRCLE" => {
            let r = num(g, 40.0, 0.0);
            if r > 0.0 { Ent::Circle { c: [x(num(g, 10.0, 0.0)), num(g, 20.0, 0.0)], r } } else { Ent::Nothing }
        }
        "ARC" => {
            let mut a0 = num(g, 50.0, 0.0) * PI / 180.0;
            let mut a1 = num(g, 51.0, 0.0) * PI / 180.0;
            if f {
                (a0, a1) = (PI - a1, PI - a0);
            }
            let r = num(g, 40.0, 0.0);
            if r > 0.0 { Ent::Arc { c: [x(num(g, 10.0, 0.0)), num(g, 20.0, 0.0)], r, a0, a1 } } else { Ent::Nothing }
        }
        "INSERT" => {
            let mut rot = num(g, 50.0, 0.0) * PI / 180.0;
            let mut sx = num(g, 41.0, 1.0);
            let mut xx = num(g, 10.0, 0.0);
            if f {
                xx = -xx;
                rot = PI - rot;
                sx = -sx;
            }
            let mut attribs = Vec::new();
            for (ty, ag) in follow.unwrap_or(&[]) {
                if ty == "ATTRIB"
                    && let Ent::Text(tx) = dxf_ent("ATTRIB", ag, None, false)?
                {
                    attribs.push(tx);
                }
            }
            Ent::Insert {
                block: val(g, 2.0).unwrap_or("").to_uppercase(),
                x: xx,
                y: num(g, 20.0, 0.0),
                sx,
                sy: num(g, 42.0, 1.0),
                rot,
                cols: pint(g, 70.0, "1"),
                rows: pint(g, 71.0, "1"),
                dc: num(g, 44.0, 0.0),
                dr: num(g, 45.0, 0.0),
                attribs,
            }
        }
        "ATTDEF" | "SEQEND" => Ent::Nothing,
        "ATTRIB" => {
            if to_i32(pint(g, 70.0, "0")) & 1 != 0 {
                Ent::Nothing
            } else {
                dxf_text(g, f, 74.0)
            }
        }
        "TEXT" => dxf_text(g, f, 73.0),
        "MTEXT" => {
            let mut raw = String::new();
            for (c, v) in g {
                if *c == 3.0 || *c == 1.0 {
                    raw.push_str(v);
                }
            }
            let value = clean_mtext(&raw);
            let h = num(g, 40.0, 0.0);
            if js::trim(&value).is_empty() || !(h > 0.0) {
                return Ok(Ent::Nothing);
            }
            let mut rot = num(g, 50.0, 0.0);
            if val(g, 11.0).is_some() {
                rot = atan2(num(g, 21.0, 0.0), num(g, 11.0, 0.0));
            }
            let attach = pint(g, 71.0, "1");
            let spacing = num(g, 44.0, 1.0);
            Ent::Text(Text {
                lines: value.split('\n').map(str::to_string).collect(),
                h,
                wf: 1.0,
                oblique: 0.0,
                rot,
                x: x(num(g, 10.0, 0.0)),
                y: num(g, 20.0, 0.0),
                attach: Some(if attach.is_nan() || attach == 0.0 { 1.0 } else { attach }),
                h_align: 0.0,
                v_align: 0.0,
                ax: 0.0,
                ay: 0.0,
                width: num(g, 41.0, 0.0),
                spacing: if spacing != 0.0 { spacing } else { 1.0 },
                mirror: f,
            })
        }
        "ELLIPSE" => {
            let c = [num(g, 10.0, 0.0), num(g, 20.0, 0.0)];
            let maj = [num(g, 11.0, 0.0), num(g, 21.0, 0.0)];
            ellipse_curve(c, maj, num(g, 40.0, 1.0), num(g, 41.0, 0.0), num(g, 42.0, 2.0 * PI), num(g, 230.0, 1.0) < 0.0)?
        }
        "SPLINE" => {
            let flags = pint(g, 70.0, "0");
            let degree = pint(g, 71.0, "3");
            let mut knots = Vec::new();
            let mut weights = Vec::new();
            let mut ctrl: Vec<P2> = Vec::new();
            let mut fit: Vec<P2> = Vec::new();
            for (c, v) in g {
                let xv = parse_float(v);
                if *c == 40.0 {
                    knots.push(Some(xv));
                } else if *c == 41.0 {
                    weights.push(xv);
                } else if *c == 10.0 {
                    ctrl.push([xv, 0.0]);
                } else if *c == 20.0
                    && let Some(l) = ctrl.last_mut()
                {
                    l[1] = xv;
                } else if *c == 11.0 {
                    fit.push([xv, 0.0]);
                } else if *c == 21.0
                    && let Some(l) = fit.last_mut()
                {
                    l[1] = xv;
                }
            }
            let w = if weights.len() == ctrl.len() { Some(weights.as_slice()) } else { None };
            spline_curve(degree, &Knots::List(knots), &ctrl, w, &fit, to_i32(flags) & 1 == 1)?
        }
        "3DFACE" | "SOLID" | "TRACE" => {
            let mut p: Vec<P3> = (0..4u8)
                .map(|k| {
                    let k = f64::from(k);
                    [num(g, 10.0 + k, 0.0), num(g, 20.0 + k, 0.0), num(g, 30.0 + k, 0.0)]
                })
                .collect();
            if typ != "3DFACE" {
                for q in &mut p {
                    q[0] = x(q[0]);
                    q[2] = num(g, 38.0, q[2]);
                }
                p.swap(2, 3);
            }
            face_ent(p)
        }
        "HATCH" => {
            let loops = dxf_hatch_loops(g)?;
            if loops.is_empty() {
                Ent::Nothing
            } else if f {
                Ent::Hatch(loops.into_iter().map(|l| l.into_iter().map(|[a, b]| [-a, b]).collect()).collect())
            } else {
                Ent::Hatch(loops)
            }
        }
        "DIMENSION" | "ARC_DIMENSION" | "LARGE_RADIAL_DIMENSION" => match val(g, 2.0).filter(|b| !b.is_empty()) {
            Some(b) => Ent::Dim(b.to_uppercase()),
            None => Ent::Skip("DIMENSION".into()),
        },
        other => Ent::Skip(other.to_string()),
    })
}

/// `text.split(/\r?\n/)`.
fn split_lines(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = text.split('\n').collect();
    let n = out.len();
    for (i, s) in out.iter_mut().enumerate() {
        if i.saturating_add(1) < n {
            *s = s.strip_suffix('\r').unwrap_or(s);
        }
    }
    out
}

fn is_space_name(bname: &str) -> bool {
    let lower = bname.to_ascii_lowercase();
    lower.starts_with("*model_space") || lower.starts_with("*paper_space")
}

/// `importDXF(model, text, name, {unit})` → {def, report, units}. `units` is None when the file does not say
/// ($INSUNITS 0): `unit` is used then.
#[allow(clippy::too_many_lines)]
pub fn import_dxf(model: &mut Model, text: &str, name: &str, unit: Option<&str>) -> Res<Imported> {
    if text.starts_with("AutoCAD Binary DXF") {
        return Err(err("m3d.io.this_is_a_binary_dxf_save"));
    }
    let raw = split_lines(text);
    let mut pairs: Vec<Pair<'_>> = Vec::with_capacity(raw.len() / 2);
    let mut i = 0usize;
    while i.saturating_add(1) < raw.len() {
        let code = parse_int(raw.get(i).copied().unwrap_or(""));
        if code.is_nan() {
            return Err(crate::i18n::err_with("m3d.io.dxf_code", &[("line", &(i.saturating_add(1)).to_string())]));
        }
        pairs.push((code, raw.get(i.saturating_add(1)).copied().unwrap_or("")));
        i = i.saturating_add(2);
    }
    let len = pairs.len();
    let at = |i: usize| -> (f64, &str) { pairs.get(i).copied().unwrap_or((0.0, "EOF")) };
    let is_end = |i: usize, names: &[&str]| -> bool {
        let (c, v) = at(i);
        c == 0.0 && names.contains(&js::trim(v))
    };
    let group = |i: &mut usize| -> Vec<Pair<'_>> {
        let mut g = Vec::new();
        while *i < len && at(*i).0 != 0.0 {
            if let Some(p) = pairs.get(*i) {
                g.push(*p);
            }
            *i = i.saturating_add(1);
        }
        g
    };
    let read_list = |i: &mut usize, top: bool| -> Res<Vec<Ent>> {
        let mut list = Vec::new();
        while *i < len {
            let (c, v) = at(*i);
            if c != 0.0 {
                *i = i.saturating_add(1);
                continue;
            }
            let typ = js::trim(v);
            if matches!(typ, "ENDSEC" | "ENDBLK" | "EOF") {
                break;
            }
            *i = i.saturating_add(1);
            let g = group(i);
            let mut follow: Option<Vec<(String, Vec<Pair<'_>>)>> = None;
            if typ == "POLYLINE" || (typ == "INSERT" && val(&g, 66.0) == Some("1")) {
                let mut fl = Vec::new();
                while *i < len && at(*i).0 == 0.0 && matches!(js::trim(at(*i).1), "VERTEX" | "ATTRIB") {
                    let tt = js::trim(at(*i).1).to_string();
                    *i = i.saturating_add(1);
                    fl.push((tt, group(i)));
                }
                if is_end(*i, &["SEQEND"]) {
                    *i = i.saturating_add(1);
                    group(i);
                }
                follow = Some(fl);
            }
            list.push(dxf_ent(typ, &g, follow.as_deref(), top)?);
        }
        Ok(list)
    };
    let mut plan = Plan::default();
    let mut i = 0usize;
    while i < len {
        if is_end(i, &["SECTION"]) {
            let sec = js::trim(pairs.get(i.saturating_add(1)).map_or("", |p| p.1)).to_string();
            i = i.saturating_add(2);
            if sec == "HEADER" {
                let mut var_name = String::new();
                while i < len && !is_end(i, &["ENDSEC"]) {
                    let (c, v) = at(i);
                    if c == 9.0 {
                        var_name = js::trim(v).to_string();
                    } else if var_name == "$INSUNITS" && c == 70.0 {
                        let u = parse_int(v);
                        plan.ins_units = if u.is_nan() || u == 0.0 { 0.0 } else { u };
                    }
                    i = i.saturating_add(1);
                }
                i = i.saturating_add(1);
            } else if sec == "BLOCKS" {
                while i < len && !is_end(i, &["ENDSEC"]) {
                    if is_end(i, &["BLOCK"]) {
                        i = i.saturating_add(1);
                        let g = group(&mut i);
                        let bname = val(&g, 2.0).unwrap_or("").to_string();
                        let base = [num(&g, 10.0, 0.0), num(&g, 20.0, 0.0)];
                        let list = read_list(&mut i, false)?;
                        if is_end(i, &["ENDBLK"]) {
                            i = i.saturating_add(1);
                            group(&mut i);
                        }
                        if !bname.is_empty() && !is_space_name(&bname) {
                            plan.blocks.insert(bname.to_uppercase(), Block { base, ents: list });
                        }
                    } else {
                        i = i.saturating_add(1);
                    }
                }
                i = i.saturating_add(1);
            } else if sec == "ENTITIES" {
                plan.ents = read_list(&mut i, true)?;
                i = i.saturating_add(1);
            } else {
                while i < len && !is_end(i, &["ENDSEC"]) {
                    i = i.saturating_add(1);
                }
                i = i.saturating_add(1);
            }
        } else if is_end(i, &["EOF"]) {
            break;
        } else {
            i = i.saturating_add(1);
        }
    }
    site_edges(model, &plan, name, unit)
}

// ---------------------------------------------------------------------------------------------------------------
// DWG (the cc-dwg drawing model as JSON)

fn xy(p: Option<&Value>) -> Option<P2> {
    match (crate::jsx::finite(get(p, "x")), crate::jsx::finite(get(p, "y"))) {
        (Some(x), Some(y)) if truthy(p) => Some([x, y]),
        _ => None,
    }
}
/// `Number(v)` for a JSON value (a relational comparison's ToNumber).
fn tonum(v: Option<&Value>) -> f64 {
    cc_m3d::file::to_number(v)
}
/// JS truthiness of `v.flags & bit`.
fn flag(v: Option<&Value>, bit: i32) -> bool {
    to_i32(tonum(v)) & bit != 0
}

/// `dwgEnt(e)`.
#[allow(clippy::too_many_lines)]
fn dwg_ent(e: &Value) -> Res<Ent> {
    let ev = Some(e);
    if !matches!(e, Value::Object(_) | Value::Array(_)) || truthy(get(ev, "invisible")) {
        return Ok(Ent::Nothing);
    }
    let ext = get(ev, "extrusion");
    let f = truthy(ext) && tonum(get(ext, "z")) < -0.5;
    let fx = |p: Option<P2>| p.map(|p| if f { [-p[0], p[1]] } else { p });
    let verts = |list: Option<&Value>, flip: bool, skip_frame: bool| -> Vec<(P2, f64)> {
        let mut out = Vec::new();
        for v in arr(list) {
            if !truthy(Some(v)) || (skip_frame && flag(get(Some(v), "flags"), 16)) {
                continue;
            }
            let q = xy(Some(v));
            let p = if flip { fx(q) } else { q };
            let b = if flip { (if f { -1.0 } else { 1.0 }) * fin(get(Some(v), "bulge"), 0.0) } else { 0.0 };
            if let Some(p) = p {
                out.push((p, b));
            }
        }
        out
    };
    let typ = get(ev, "type").and_then(Value::as_str);
    let s = |k: &str| get(ev, k);
    Ok(match typ {
        Some("LINE") => match (xy(s("start")), xy(s("end"))) {
            (Some(a), Some(b)) => Ent::Line(a, b),
            _ => Ent::Nothing,
        },
        Some(ty @ ("LWPOLYLINE" | "POLYLINE_2D" | "POLYLINE_3D")) => {
            let vs = verts(s("vertices"), ty != "POLYLINE_3D", ty != "LWPOLYLINE");
            if vs.len() >= 2 { Ent::Poly { vs, closed: truthy(s("closed")) } } else { Ent::Nothing }
        }
        Some("POLYLINE_PFACE" | "POLYLINE_MESH") => Ent::Skip("POLYLINE (mesh)".into()),
        Some("CIRCLE") => match fx(xy(s("center"))) {
            Some(c) if tonum(s("radius")) > 0.0 => Ent::Circle { c, r: tonum(s("radius")) },
            _ => Ent::Nothing,
        },
        Some("ARC") => {
            let c = fx(xy(s("center")));
            let mut a0 = fin(s("startAngle"), 0.0);
            let mut a1 = fin(s("endAngle"), 0.0);
            if f {
                (a0, a1) = (PI - a1, PI - a0);
            }
            match c {
                Some(c) if tonum(s("radius")) > 0.0 => Ent::Arc { c, r: tonum(s("radius")), a0, a1 },
                _ => Ent::Nothing,
            }
        }
        Some("INSERT" | "MINSERT") => {
            let p = xy(s("insert"));
            let (Some(p), true) = (p, truthy(s("block"))) else { return Ok(Ent::Nothing) };
            let mut x = p[0];
            let mut rot = fin(s("rotation"), 0.0);
            let sx0 = fin(get(s("scale"), "x"), 1.0);
            let mut sx = if sx0 != 0.0 { sx0 } else { 1.0 };
            if f {
                x = -x;
                rot = PI - rot;
                sx = -sx;
            }
            let sy0 = fin(get(s("scale"), "y"), 1.0);
            let mut attribs = Vec::new();
            for a in arr(s("attribs")) {
                if let Ent::Text(tx) = dwg_ent(a)? {
                    attribs.push(tx);
                }
            }
            Ent::Insert {
                block: crate::jsx::to_js_string(s("block")).to_uppercase(),
                x,
                y: p[1],
                sx,
                sy: if sy0 != 0.0 { sy0 } else { 1.0 },
                rot,
                cols: fin(s("cols"), 1.0),
                rows: fin(s("rows"), 1.0),
                dc: fin(s("colSpacing"), 0.0),
                dr: fin(s("rowSpacing"), 0.0),
                attribs,
            }
        }
        Some("ATTDEF") => Ent::Nothing,
        Some(ty @ ("ATTRIB" | "TEXT")) => {
            if ty == "ATTRIB" && flag(s("flags"), 1) {
                return Ok(Ent::Nothing);
            }
            let raw = match s("value") {
                None | Some(Value::Null) => String::new(),
                Some(v) => crate::jsx::to_js_string(Some(v)),
            };
            let value = clean_dxf_text(&raw);
            let h = tonum(s("height"));
            if js::trim(&value).is_empty() || !(h > 0.0) {
                return Ok(Ent::Nothing);
            }
            let ins = fx(xy(s("insert")));
            let al = fx(xy(s("align")).or_else(|| xy(s("insert"))));
            match (ins, al) {
                (Some(ins), al) => {
                    let al = al.unwrap_or(ins);
                    let wf = fin(s("widthFactor"), 1.0);
                    Ent::Text(Text {
                        lines: vec![value],
                        h,
                        wf: if wf != 0.0 { wf } else { 1.0 },
                        oblique: fin(s("oblique"), 0.0),
                        rot: fin(s("rotation"), 0.0),
                        x: ins[0],
                        y: ins[1],
                        attach: None,
                        h_align: fin(s("hAlign"), 0.0),
                        v_align: fin(s("vAlign"), 0.0),
                        ax: al[0],
                        ay: al[1],
                        width: f64::NAN,
                        spacing: 1.0,
                        mirror: f,
                    })
                }
                _ => Ent::Nothing,
            }
        }
        Some("MTEXT") => {
            let raw = match s("value") {
                None | Some(Value::Null) => String::new(),
                Some(v) => crate::jsx::to_js_string(Some(v)),
            };
            let value = clean_mtext(&raw);
            let ins = xy(s("insert"));
            let h = tonum(s("height"));
            let (false, true, Some(ins)) = (js::trim(&value).is_empty(), h > 0.0, ins) else { return Ok(Ent::Nothing) };
            let xa = s("xAxis");
            let rot = if truthy(xa) && (truthy(get(xa, "x")) || truthy(get(xa, "y"))) {
                atan2(tonum(get(xa, "y")), tonum(get(xa, "x")))
            } else {
                fin(s("rotation"), 0.0)
            };
            let attach = fin(s("attach"), 1.0);
            let spacing = fin(s("lineSpacing"), 1.0);
            Ent::Text(Text {
                lines: value.split('\n').map(str::to_string).collect(),
                h,
                wf: 1.0,
                oblique: 0.0,
                rot,
                x: ins[0],
                y: ins[1],
                attach: Some(if attach != 0.0 { attach } else { 1.0 }),
                h_align: 0.0,
                v_align: 0.0,
                ax: 0.0,
                ay: 0.0,
                width: fin(s("width"), 0.0),
                spacing: if spacing != 0.0 { spacing } else { 1.0 },
                mirror: false,
            })
        }
        Some("ELLIPSE") => match (xy(s("center")), xy(s("majorAxis"))) {
            (Some(c), Some(m)) => ellipse_curve(c, m, fin(s("ratio"), 1.0), fin(s("startParam"), 0.0), fin(s("endParam"), 2.0 * PI), f)?,
            _ => Ent::Nothing,
        },
        Some("SPLINE") => {
            let pts = |a: Option<&Value>| -> Vec<P2> { arr(a).iter().filter_map(|v| xy(Some(v))).collect() };
            let knots = knots_of(s("knots"), true);
            let weights: Option<Vec<f64>> = s("weights").and_then(Value::as_array).map(|a| a.iter().map(|v| weight_of(Some(v))).collect());
            spline_curve(fin(s("degree"), 3.0), &knots, &pts(s("control")), weights.as_deref(), &pts(s("fit")), truthy(s("closed")))?
        }
        Some(ty @ ("3DFACE" | "SOLID" | "TRACE")) => {
            let pv = s("points");
            let list: &[Value] = match pv {
                Some(Value::Array(a)) => a,
                v if !truthy(v) => &[],
                // `(e.points || []).slice(0, 4).map(…)` on something that is not an array
                _ => return Err(cc_m3d::Error::Type),
            };
            let mut p: Vec<Option<P3>> =
                list.iter().take(4).map(|q| xy(Some(q)).map(|[a, b]| [a, b, fin(get(Some(q), "z"), 0.0)])).collect();
            if p.len() < 3 || p.iter().any(Option::is_none) {
                return Ok(Ent::Nothing);
            }
            if ty != "3DFACE" {
                for q in p.iter_mut().flatten() {
                    if f {
                        q[0] = -q[0];
                    }
                    q[2] = fin(s("elevation"), q[2]);
                }
                if p.len() == 4 {
                    p.swap(2, 3);
                }
            }
            face_ent(p.into_iter().flatten().collect())
        }
        Some("HATCH") => {
            let mut loops: Vec<Vec<P2>> = Vec::new();
            for l in arr(s("loops")) {
                if l.is_null() {
                    return Err(cc_m3d::Error::Type);
                }
                let lv = Some(l);
                let pl = get(lv, "polyline");
                if truthy(pl)
                    && let Some(vs) = get(pl, "vertices").and_then(Value::as_array)
                {
                    let mut pts: Vec<(Option<P2>, f64)> = Vec::new();
                    for v in vs {
                        if v.is_null() {
                            return Err(cc_m3d::Error::Type);
                        }
                        if let Some(p) = xy(Some(v)) {
                            pts.push((Some(p), fin(get(Some(v), "bulge"), 0.0)));
                        }
                    }
                    loops.push(loop_points(LoopIn::Poly(&pts))?);
                } else if let Some(es) = get(lv, "edges").and_then(Value::as_array) {
                    let mut edges: Vec<HEdge> = Vec::new();
                    for d in es {
                        if d.is_null() {
                            return Err(cc_m3d::Error::Type);
                        }
                        let dv = Some(d);
                        let dt = get(dv, "type").and_then(Value::as_str);
                        let ccw = !matches!(get(dv, "ccw"), Some(Value::Bool(false)));
                        match dt {
                            Some("line") => {
                                if let (Some(a), Some(b)) = (xy(get(dv, "start")), xy(get(dv, "end"))) {
                                    edges.push(HEdge::Line(a, b));
                                }
                            }
                            Some("arc") => {
                                if let Some(c) = xy(get(dv, "center")) {
                                    edges.push(HEdge::Arc {
                                        c,
                                        r: fin(get(dv, "radius"), 0.0),
                                        a0: fin(get(dv, "startAngle"), 0.0),
                                        a1: fin(get(dv, "endAngle"), 0.0),
                                        ccw,
                                    });
                                }
                            }
                            Some("ellipse") => {
                                if let Some(c) = xy(get(dv, "center")) {
                                    edges.push(HEdge::Ell {
                                        c,
                                        maj: xy(get(dv, "majorAxis")),
                                        ratio: fin(get(dv, "ratio"), 1.0),
                                        a0: fin(get(dv, "startAngle"), 0.0),
                                        a1: fin(get(dv, "endAngle"), 0.0),
                                        ccw,
                                    });
                                }
                            }
                            Some("spline") => {
                                let ctrl = match get(dv, "control") {
                                    Some(Value::Array(a)) => a.iter().filter_map(|v| xy(Some(v))).collect(),
                                    v if !truthy(v) => Vec::new(),
                                    _ => return Err(cc_m3d::Error::Type),
                                };
                                let wv = get(dv, "weights");
                                let weights = if truthy(wv) {
                                    match wv {
                                        Some(Value::Array(a)) => Some(a.iter().map(|v| weight_of(Some(v))).collect()),
                                        // a non-array weights value: every lookup is undefined → 1
                                        _ => Some(Vec::new()),
                                    }
                                } else {
                                    None
                                };
                                edges.push(HEdge::Spline {
                                    degree: fin(get(dv, "degree"), 3.0),
                                    knots: knots_of(get(dv, "knots"), false),
                                    ctrl,
                                    weights,
                                });
                            }
                            _ => {}
                        }
                    }
                    loops.push(loop_points(LoopIn::Edges(&edges))?);
                }
            }
            loops.retain(|l| l.len() >= 3);
            if loops.is_empty() {
                Ent::Nothing
            } else if f {
                Ent::Hatch(loops.into_iter().map(|l| l.into_iter().map(|[a, b]| [-a, b]).collect()).collect())
            } else {
                Ent::Hatch(loops)
            }
        }
        _ => {
            let ty_s = crate::jsx::to_js_string(get(ev, "type"));
            let dimish = ty_s.starts_with("DIMENSION_") || ty_s == "ARC_DIMENSION" || ty_s == "LARGE_RADIAL_DIMENSION";
            if dimish && truthy(s("block")) {
                return Ok(Ent::Dim(crate::jsx::to_js_string(s("block")).to_uppercase()));
            }
            let what = if typ == Some("UNKNOWN") {
                if truthy(s("dxfName")) { crate::jsx::to_js_string(s("dxfName")) } else { "unknown".into() }
            } else if ty_s.contains("DIMENSION") {
                "DIMENSION".into()
            } else if truthy(get(ev, "type")) {
                ty_s
            } else {
                "unknown".into()
            };
            Ent::Skip(crate::util::slice16(&what, 40))
        }
    })
}

/// A spline weight as the JS uses it: `W[i] || 1` (falsy → 1, else its number).
fn weight_of(v: Option<&Value>) -> f64 {
    if truthy(v) { tonum(v) } else { 1.0 }
}
/// Knots: an array (its numbers; anything else is not finite), or (`d.knots || []` / `Array.isArray(k) ? k : []`) not one.
fn knots_of(v: Option<&Value>, array_only: bool) -> Knots {
    match v {
        Some(Value::Array(a)) => Knots::List(a.iter().map(|x| crate::jsx::num(Some(x))).collect()),
        _ if array_only => Knots::List(Vec::new()),
        v if !truthy(v) => Knots::List(Vec::new()),
        Some(Value::String(s)) => Knots::Other(Some(s.encode_utf16().count())),
        _ => Knots::Other(None),
    }
}

/// `importDWG(model, dwg, name, {unit})` → {def, report, units}, exactly like importDXF. Model space only; blocks
/// expanded.
pub fn import_dwg(model: &mut Model, dwg: &Value, name: &str, unit: Option<&str>) -> Res<Imported> {
    let dv = Some(dwg);
    let Some(blocks) = get(dv, "blocks").and_then(Value::as_array) else { return Err(err("m3d.io.this_dwg_could_not_be_read")) };
    let mut plan = Plan::default();
    let mut ms: Option<&Value> = None;
    let mut paper = 0usize;
    for b in blocks {
        let bv = Some(b);
        let nm = get(bv, "name");
        let bn = if truthy(nm) { crate::jsx::to_js_string(nm) } else { String::new() };
        if bn.eq_ignore_ascii_case("*model_space") {
            ms = Some(b);
            continue;
        }
        if bn.len() >= 12 && bn.get(..12).is_some_and(|p| p.eq_ignore_ascii_case("*paper_space")) {
            paper = paper.saturating_add(get(bv, "entities").and_then(Value::as_array).map_or(0, Vec::len));
            continue;
        }
        if !bn.is_empty() {
            let base = get(bv, "base");
            let mut ents = Vec::new();
            for e in arr(get(bv, "entities")) {
                ents.push(dwg_ent(e)?);
            }
            plan.blocks.insert(bn.to_uppercase(), Block { base: [fin(get(base, "x"), 0.0), fin(get(base, "y"), 0.0)], ents });
        }
    }
    for e in arr(get(ms, "entities")) {
        plan.ents.push(dwg_ent(e)?);
    }
    for _ in 0..paper.min(MAX_SEGS) {
        plan.ents.push(Ent::Paper);
    }
    let iu = get(get(dv, "header"), "INSUNITS");
    plan.ins_units = crate::jsx::int(iu).unwrap_or(0.0);
    site_edges(model, &plan, name, unit)
}

// ---------------------------------------------------------------------------------------------------------------
// siteEdges

fn dxf_unit(u: f64) -> Option<(&'static str, f64)> {
    match u {
        1.0 => Some(("in", 0.0254)),
        2.0 => Some(("ft", 0.3048)),
        3.0 => Some(("mi", 1609.344)),
        4.0 => Some(("mm", 0.001)),
        5.0 => Some(("cm", 0.01)),
        6.0 => Some(("m", 1.0)),
        7.0 => Some(("km", 1000.0)),
        10.0 => Some(("yd", 0.9144)),
        _ => None,
    }
}

type Aff = [f64; 6];
#[inline]
fn tp(m: &Aff, p: P2) -> P2 {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}
#[inline]
fn tmul(m: &Aff, n: &Aff) -> Aff {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

fn area2(pts: &[P2]) -> f64 {
    let mut a = 0.0;
    let n = pts.len();
    for (i, p) in pts.iter().enumerate() {
        let q = pts.get(i.saturating_add(1).checked_rem(n).unwrap_or(0)).copied().unwrap_or(*p);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}
fn in_poly(p: P2, pts: &[P2]) -> bool {
    let mut c = false;
    let n = pts.len();
    if n == 0 {
        return false;
    }
    let mut j = n.saturating_sub(1);
    for i in 0..n {
        let (a, b) = (pts.get(i).copied().unwrap_or([0.0; 2]), pts.get(j).copied().unwrap_or([0.0; 2]));
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < ((b[0] - a[0]) * (p[1] - a[1])) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}

/// A polygon for meshDef: a face's corners, or a hatch's loops (outer, holes).
enum SitePoly {
    Face(Vec<P3>),
    Loops(Vec<Vec<P3>>),
}

struct Site<'p> {
    plan: &'p Plan,
    k: f64,
    segs: Vec<[P3; 2]>,
    polys: Vec<SitePoly>,
    text_segs: f64,
    skipped: Map<String, Value>,
    counts: [f64; 13],
    draws: u64,
}

// counts: lines, polylines, circles, arcs, inserts, splines, ellipses, texts, hatches, faces3d, dimensions, (unused), paperSpace
const C_LINES: usize = 0;
const C_POLYS: usize = 1;
const C_CIRCLES: usize = 2;
const C_ARCS: usize = 3;
const C_INSERTS: usize = 4;
const C_SPLINES: usize = 5;
const C_ELLIPSES: usize = 6;
const C_TEXTS: usize = 7;
const C_HATCHES: usize = 8;
const C_FACES3D: usize = 9;
const C_DIMS: usize = 10;
const C_PAPER: usize = 12;

impl Site<'_> {
    fn push_poly(&mut self, p: SitePoly) -> Res<()> {
        if self.polys.len() >= MAX_POLYS {
            return Err(err("m3d.io.this_drawing_has_more_than_200"));
        }
        self.polys.push(p);
        Ok(())
    }
    fn count(&mut self, k: usize) {
        if let Some(c) = self.counts.get_mut(k) {
            *c += 1.0;
        }
    }
    fn seg(&mut self, m: &Aff, p: P2, q: P2) -> Res<()> {
        let a = tp(m, p);
        let b = tp(m, q);
        if a[0] == b[0] && a[1] == b[1] {
            return Ok(());
        }
        if !(a[0].is_finite() && a[1].is_finite() && b[0].is_finite() && b[1].is_finite()) {
            return Ok(());
        }
        let mx = a[0].abs().max(a[1].abs()).max(b[0].abs()).max(b[1].abs());
        if mx * self.k >= 1e7 {
            return Err(err("m3d.io.this_drawing_reaches_more_than_10"));
        }
        if self.segs.len() >= MAX_SEGS {
            return Err(err("m3d.io.this_drawing_has_more_than_2"));
        }
        let k = self.k;
        self.segs.push([[a[0] * k, a[1] * k, 0.0], [b[0] * k, b[1] * k, 0.0]]);
        Ok(())
    }
    /// `run(m, arcPts(c, r, a0, sweep))` without building the point list.
    fn arc_run(&mut self, m: &Aff, c: P2, r: f64, a0: f64, sweep: f64) -> Res<()> {
        let n = (2.0f64).max((sweep.abs() / (2.0 * PI / CIRCLE_SEGS)).ceil());
        if !(n <= MAX_ARC_POINTS) {
            return Err(err("m3d.io.this_drawing_has_more_than_2"));
        }
        let pt = |j: f64| -> P2 {
            let a = a0 + (sweep * j) / n;
            [c[0] + r * cos(a), c[1] + r * sin(a)]
        };
        let mut prev = pt(0.0);
        let mut j = 1.0;
        while j <= n {
            let cur = pt(j);
            self.seg(m, prev, cur)?;
            prev = cur;
            j += 1.0;
        }
        Ok(())
    }
    fn run(&mut self, m: &Aff, pts: &[P2]) -> Res<()> {
        for w in pts.windows(2) {
            if let [p, q] = w {
                self.seg(m, *p, *q)?;
            }
        }
        Ok(())
    }
    /// `polyRun(m, vs, closed)` with `bulgePts`.
    fn poly_run(&mut self, m: &Aff, vs: &[(P2, f64)], closed: bool) -> Res<()> {
        let n = vs.len();
        let k = if closed { n } else { n.saturating_sub(1) };
        for j in 0..k {
            let Some(&(p, b)) = vs.get(j) else { continue };
            let q = vs.get(j.saturating_add(1).checked_rem(n).unwrap_or(0)).map_or([f64::NAN; 2], |v| v.0);
            if b == 0.0 || b.is_nan() || b.abs() < 1e-12 {
                self.seg(m, p, q)?;
                continue;
            }
            let s = (1.0 / b - b) / 2.0;
            let c = [(p[0] + q[0]) / 2.0 - s * (q[1] - p[1]) / 2.0, (p[1] + q[1]) / 2.0 + s * (q[0] - p[0]) / 2.0];
            let r = hypot(&[p[0] - c[0], p[1] - c[1]]);
            self.arc_run(m, c, r, atan2(p[1] - c[1], p[0] - c[0]), 4.0 * atan(b))?;
        }
        Ok(())
    }
    fn p3(&self, m: &Aff, p: P3) -> P3 {
        let a = tp(m, [p[0], p[1]]);
        let z = if p[2] != 0.0 && !p[2].is_nan() { p[2] } else { 0.0 };
        [a[0] * self.k, a[1] * self.k, z * self.k]
    }

    /// `text(m, e)`: the lines in the stroke font, placed by the entity's alignment (TEXT) or attachment (MTEXT).
    fn text(&mut self, m: &Aff, e: &Text) -> Res<()> {
        let sp = if e.spacing != 0.0 && !e.spacing.is_nan() { e.spacing } else { 1.0 };
        let lh = e.h * 1.667 * sp;
        let mut rows: Vec<String> = Vec::new();
        for raw in &e.lines {
            // MTEXT with a reference width wraps at spaces
            if e.width > 0.0 && e.attach.is_some() {
                let mut cur = String::new();
                for w in raw.split(' ') {
                    let try_s = if cur.is_empty() { w.to_string() } else { format!("{cur} {w}") };
                    if !cur.is_empty() && text_width(&try_s, e.h, 1.0) > e.width {
                        rows.push(std::mem::replace(&mut cur, w.to_string()));
                    } else {
                        cur = try_s;
                    }
                }
                rows.push(cur);
            } else {
                rows.push(raw.clone());
            }
        }
        let mut shaped: Vec<crate::strokefont::Strokes> = rows.iter().map(|r| text_strokes(r, e.h, e.wf, e.oblique)).collect();
        let (mut ox, mut oy, mut px, mut py, mut rot) = (0.0f64, 0.0f64, e.x, e.y, e.rot);
        let w0 = shaped.first().map_or(0.0, |s| s.width);
        let w0 = if w0 != 0.0 && !w0.is_nan() { w0 } else { 0.0 };
        if let Some(att) = e.attach {
            // MTEXT: 1 2 3 top, 4 5 6 middle, 7 8 9 bottom; left / centre / right
            let col = (att - 1.0) % 3.0;
            let row = ((att - 1.0) / 3.0).floor();
            let nr = rows.len() as f64;
            let hh = (nr - 1.0) * lh + e.h;
            oy = if row == 0.0 {
                -e.h
            } else if row == 1.0 {
                hh / 2.0 - e.h
            } else {
                (nr - 1.0) * lh
            };
            ox = col;
        } else if (e.h_align != 0.0 && !e.h_align.is_nan()) || (e.v_align != 0.0 && !e.v_align.is_nan()) {
            px = e.ax;
            py = e.ay;
            if e.h_align == 3.0 || e.h_align == 5.0 {
                // aligned / fit: between the two points
                let l = hypot(&[e.ax - e.x, e.ay - e.y]);
                if l > 0.0 && w0 > 0.0 {
                    rot = atan2(e.ay - e.y, e.ax - e.x);
                    let wf = if e.wf != 0.0 && !e.wf.is_nan() { e.wf } else { 1.0 };
                    let h = if e.h_align == 3.0 { e.h * l / w0 } else { e.h };
                    let s0 = text_strokes(
                        rows.first().map_or("", String::as_str),
                        h,
                        wf * if e.h_align == 5.0 { l / w0 } else { 1.0 },
                        e.oblique,
                    );
                    if let Some(first) = shaped.first_mut() {
                        *first = s0;
                    }
                }
                px = e.x;
                py = e.y;
            } else {
                ox = if e.h_align == 1.0 || e.h_align == 4.0 {
                    -w0 / 2.0
                } else if e.h_align == 2.0 {
                    -w0
                } else {
                    0.0
                };
                oy = if e.h_align == 4.0 || e.v_align == 2.0 {
                    -e.h / 2.0
                } else if e.v_align == 3.0 {
                    -e.h
                } else if e.v_align == 1.0 {
                    e.h / 3.0
                } else {
                    0.0
                };
            }
        }
        let cs = cos(rot);
        let sn = sin(rot);
        let mi = if e.mirror { -1.0 } else { 1.0 };
        let mm = tmul(m, &[cs * mi, sn * mi, -sn, cs, px, py]);
        self.count(C_TEXTS);
        let glyph_key = t("m3d.io.character_no_glyph_drawn_as_a", &[]);
        for (r, sh) in shaped.iter().enumerate() {
            let dx = if e.attach.is_some() {
                if ox == 1.0 {
                    -sh.width / 2.0
                } else if ox == 2.0 {
                    -sh.width
                } else {
                    0.0
                }
            } else {
                ox
            };
            let dy = oy - r as f64 * lh;
            if sh.missing != 0.0 {
                skip(&mut self.skipped, &glyph_key, sh.missing);
            }
            for l in &sh.lines {
                if self.text_segs + l.len() as f64 > MAX_TEXT_SEGS {
                    skip(&mut self.skipped, &t("m3d.io.text_over_the_text_limit", &[]), 1.0);
                    break;
                }
                self.text_segs += l.len() as f64 - 1.0;
                let pts: Vec<P2> = l.iter().map(|&[x, y]| [x + dx, y + dy]).collect();
                self.run(&mm, &pts)?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn draw(&mut self, list: &[Ent], m: &Aff, depth: u32) -> Res<()> {
        for e in list {
            self.draws = self.draws.saturating_add(1);
            if self.draws > MAX_DRAW {
                return Err(err("m3d.io.this_drawing_has_more_than_2"));
            }
            match e {
                Ent::Paper => self.count(C_PAPER),
                Ent::Skip(w) => skip(&mut self.skipped, w, 1.0),
                Ent::Nothing => {}
                Ent::Line(a, b) => {
                    self.count(C_LINES);
                    self.seg(m, *a, *b)?;
                }
                Ent::Poly { vs, closed } => {
                    self.count(C_POLYS);
                    self.poly_run(m, vs, *closed)?;
                }
                Ent::Circle { c, r } => {
                    self.count(C_CIRCLES);
                    self.arc_run(m, *c, *r, 0.0, 2.0 * PI)?;
                }
                Ent::Arc { c, r, a0, a1 } => {
                    let sw = wrap_sweep(a1 - a0).ok_or_else(|| err("m3d.io.this_drawing_has_more_than_2"))?;
                    self.count(C_ARCS);
                    self.arc_run(m, *c, *r, *a0, sw)?;
                }
                Ent::Curve { spline, pts, closed } => {
                    self.count(if *spline { C_SPLINES } else { C_ELLIPSES });
                    if *closed {
                        let mut p = pts.clone();
                        if let Some(&f) = pts.first() {
                            p.push(f);
                        } else {
                            p.push([f64::NAN; 2]);
                        }
                        self.run(m, &p)?;
                    } else {
                        self.run(m, pts)?;
                    }
                }
                Ent::Text(tx) => self.text(m, tx)?,
                Ent::Face(pts) => {
                    self.count(C_FACES3D);
                    let p: Vec<P3> = pts.iter().map(|p| self.p3(m, *p)).collect();
                    self.push_poly(SitePoly::Face(p))?;
                }
                Ent::Hatch(loops) => {
                    self.count(C_HATCHES);
                    // which loops are holes: a loop inside an odd number of others
                    let mut l: Vec<(usize, f64)> = loops.iter().enumerate().map(|(i, pts)| (i, area2(pts).abs())).collect();
                    js::sort_by(&mut l, |x, y| y.1 - x.1);
                    let mut outers: Vec<(usize, Vec<usize>)> = Vec::new();
                    for &(li, la) in &l {
                        let lp0 = loops.get(li).and_then(|p| p.first()).copied().unwrap_or([f64::NAN; 2]);
                        let mut inside: Vec<(usize, f64)> = l
                            .iter()
                            .filter(|&&(oi, oa)| oi != li && oa > la && in_poly(lp0, loops.get(oi).map_or(&[][..], Vec::as_slice)))
                            .copied()
                            .collect();
                        if inside.len().is_multiple_of(2) {
                            outers.push((li, Vec::new()));
                        } else {
                            js::sort_by(&mut inside, |x, y| x.1 - y.1);
                            if let Some(&(host, _)) = inside.first()
                                && let Some(o) = outers.iter_mut().find(|q| q.0 == host)
                            {
                                o.1.push(li);
                            }
                        }
                    }
                    for (ol, holes) in outers {
                        let mut lps = Vec::with_capacity(holes.len().saturating_add(1));
                        for q in std::iter::once(ol).chain(holes) {
                            let pts = loops.get(q).map_or(&[][..], Vec::as_slice);
                            lps.push(pts.iter().map(|p| self.p3(m, [p[0], p[1], 0.0])).collect());
                        }
                        self.push_poly(SitePoly::Loops(lps))?;
                    }
                }
                Ent::Dim(block) => match self.plan.blocks.get(block) {
                    None => skip(&mut self.skipped, &t("m3d.io.dimension_missing_block", &[]), 1.0),
                    Some(_) if depth >= 16 => skip(&mut self.skipped, &t("m3d.io.dimension_nested_too_deep", &[]), 1.0),
                    Some(b) => {
                        self.count(C_DIMS);
                        let plan = self.plan;
                        let _ = b;
                        if let Some(b) = plan.blocks.get(block) {
                            self.draw(&b.ents, m, depth.saturating_add(1))?;
                        }
                    }
                },
                Ent::Insert { block, x, y, sx, sy, rot, cols, rows, dc, dr, attribs } => {
                    let plan = self.plan;
                    match plan.blocks.get(block) {
                        None => skip(&mut self.skipped, &t("m3d.io.insert_missing_block", &[]), 1.0),
                        Some(_) if depth >= 16 => skip(&mut self.skipped, &t("m3d.io.insert_nested_too_deep", &[]), 1.0),
                        Some(b) => {
                            self.count(C_INSERTS);
                            let cs = cos(*rot);
                            let sn = sin(*rot);
                            // translate(x,y) · rotate · scale · translate(−base)
                            let rs = [cs * sx, sn * sx, -sn * sy, cs * sy, *x, *y];
                            let local = tmul(&rs, &[1.0, 0.0, 0.0, 1.0, -b.base[0], -b.base[1]]);
                            let or1 = |v: f64| if v != 0.0 && !v.is_nan() { v } else { 1.0 };
                            let nc = (1.0f64).max(or1(*cols)).min(1000.0);
                            let nr = (1.0f64).max(or1(*rows)).min(1000.0);
                            let mut r = 0.0;
                            while r < nr {
                                let mut c = 0.0;
                                while c < nc {
                                    let off = [cs * c * dc - sn * r * dr, sn * c * dc + cs * r * dr];
                                    let mm = tmul(m, &tmul(&[1.0, 0.0, 0.0, 1.0, off[0], off[1]], &local));
                                    self.draw(&b.ents, &mm, depth.saturating_add(1))?;
                                    c += 1.0;
                                }
                                r += 1.0;
                            }
                            // attribute values are placed in the insert's own space
                            for a in attribs {
                                self.text(m, a)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// `siteEdges(model, plan, name, {unit})`.
pub fn site_edges(model: &mut Model, plan: &Plan, name: &str, unit: Option<&str>) -> Res<Imported> {
    let mut units: Option<String> = None;
    let mut k = match unit {
        Some("mm") => 0.001,
        Some("cm") => 0.01,
        Some("m") => 1.0,
        Some("in") => 0.0254,
        Some("ft") => 0.3048,
        _ => 1.0,
    };
    if let Some((u, f)) = dxf_unit(plan.ins_units) {
        units = Some(u.to_string());
        k = f;
    }
    let mut s = Site { plan, k, segs: Vec::new(), polys: Vec::new(), text_segs: 0.0, skipped: Map::new(), counts: [0.0; 13], draws: 0 };
    s.draw(&plan.ents, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0)?;
    if s.polys.len() > MAX_POLYS {
        return Err(err("m3d.io.this_drawing_has_more_than_200"));
    }
    let nm = clean_name(name);
    let id = if s.polys.is_empty() {
        edges_def_in(model, &s.segs, Some(&nm))
    } else {
        let polys: Vec<PolyIn> = s
            .polys
            .drain(..)
            .map(|p| match p {
                SitePoly::Face(pts) => PolyIn::outline(pts),
                SitePoly::Loops(loops) => PolyIn { loops, color: None },
            })
            .collect();
        mesh_def_in(model, &polys, Some(&nm), "group", &s.segs)
    };
    let (ne, nf) = model.def(&id).map_or((0, 0), |d| (d.edges.len(), d.faces.len()));
    let c = s.counts;
    let mut report = Map::new();
    let names = ["lines", "polylines", "circles", "arcs", "inserts", "splines", "ellipses", "texts", "hatches", "faces3d", "dimensions"];
    for (i, n) in names.iter().enumerate() {
        report.insert((*n).into(), Value::from(c.get(i).copied().unwrap_or(0.0)));
    }
    report.insert("edges".into(), Value::from(ne as f64));
    report.insert("faces".into(), Value::from(nf as f64));
    report.insert("paperSpace".into(), Value::from(c.get(C_PAPER).copied().unwrap_or(0.0)));
    report.insert("skipped".into(), Value::Object(s.skipped));
    let _ = is_space;
    Ok(Imported { def: id, report, units })
}
