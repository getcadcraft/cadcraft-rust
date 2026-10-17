//! geom.js — editing geometry: grips, applying a grip drag, moving, simplifying freehand strokes, picking through
//! the display list, and the 2-D affine helpers with the two-point similarity used to align sheets.

use crate::js::{JsMath, hypot, max, min, round, sign};
use crate::model::{Markup, norm_rect};
use crate::shapes::{Item, NOTE_SIZE, ShapeEnv, list_bounds, list_distance, shapes};
use serde_json::{Map, Value};
use std::f64::consts::PI;

pub type P = [f64; 2];

pub const BOX_TYPES: [&str; 8] = ["rect", "ellipse", "text", "stamp", "image", "legend", "cloudrect", "callout"];
pub const VERTEX_TYPES: [&str; 11] = ["space", "line", "arrow", "arc", "angle", "measure", "polyline", "polygon", "cloud", "area", "count"];
pub const FREE_TYPES: [&str; 2] = ["pen", "highlight"];

const BOX_GRIPS: [(&str, f64, f64); 8] = [
    ("nw", 0.0, 1.0),
    ("n", 0.5, 1.0),
    ("ne", 1.0, 1.0),
    ("e", 1.0, 0.5),
    ("se", 1.0, 0.0),
    ("s", 0.5, 0.0),
    ("sw", 0.0, 0.0),
    ("w", 0.0, 0.5),
];

/// `pointsBounds(pts)`.
pub fn points_bounds(pts: &[P]) -> [f64; 4] {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        if p[0] < x0 {
            x0 = p[0];
        }
        if p[1] < y0 {
            y0 = p[1];
        }
        if p[0] > x1 {
            x1 = p[0];
        }
        if p[1] > y1 {
            y1 = p[1];
        }
    }
    [x0, y0, x1, y1]
}

/// A grip.
#[derive(Debug, Clone, PartialEq)]
pub struct Grip {
    pub key: String,
    pub kind: &'static str,
    /// the vertex index (vertex / tip / knee grips)
    pub i: Option<usize>,
    pub x: f64,
    pub y: f64,
}

/// `grips(m)`.
pub fn grips(m: &Markup) -> Vec<Grip> {
    if m.locked {
        return Vec::new();
    }
    let mut out = Vec::new();
    let bx = if BOX_TYPES.contains(&m.kind.as_str()) {
        m.rect
    } else if FREE_TYPES.contains(&m.kind.as_str()) && m.pts.as_ref().is_some_and(|p| p.len() > 1) {
        Some(points_bounds(m.pts()))
    } else {
        None
    };
    if let Some([x0, y0, x1, y1]) = bx {
        for (key, fx, fy) in BOX_GRIPS {
            out.push(Grip { key: key.into(), kind: "box", i: None, x: x0 + (x1 - x0) * fx, y: y0 + (y1 - y0) * fy });
        }
    }
    if VERTEX_TYPES.contains(&m.kind.as_str()) {
        for (i, p) in m.pts().iter().enumerate() {
            out.push(Grip { key: format!("v{i}"), kind: "vertex", i: Some(i), x: p[0], y: p[1] });
        }
    }
    if m.kind == "callout"
        && let Some(p0) = m.pts().first()
    {
        out.push(Grip { key: "tip".into(), kind: "tip", i: Some(0), x: p0[0], y: p0[1] });
        if let Some(p1) = m.pts().get(1) {
            out.push(Grip { key: "knee".into(), kind: "knee", i: Some(1), x: p1[0], y: p1[1] });
        }
    }
    out
}

/// `snap45(from, p)`.
pub fn snap45(from: P, p: P) -> P {
    let (dx, dy) = (p[0] - from[0], p[1] - from[1]);
    let len = hypot(dx, dy);
    if len == 0.0 || len.is_nan() {
        return [p[0], p[1]];
    }
    let a = round(dy.jatan2(dx) / (PI / 4.0)) * (PI / 4.0);
    [from[0] + a.jcos() * len, from[1] + a.jsin() * len]
}

/// `squareFrom(a, p)`.
pub fn square_from(a: P, p: P) -> P {
    let (dx, dy) = (p[0] - a[0], p[1] - a[1]);
    let s = max(dx.abs(), dy.abs());
    let sg = |d: f64| sign(if d == 0.0 || d.is_nan() { 1.0 } else { d });
    [a[0] + sg(dx) * s, a[1] + sg(dy) * s]
}

/// A patch from a grip drag: new points or a new rect.
#[derive(Debug, Clone, PartialEq)]
pub enum GripPatch {
    Pts(Vec<P>),
    Rect([f64; 4]),
}

impl GripPatch {
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        match self {
            GripPatch::Pts(p) => {
                m.insert("pts".into(), pts_value(p));
            }
            GripPatch::Rect(r) => {
                m.insert("rect".into(), nums_value(r));
            }
        }
        m
    }
}

fn num_value(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}
/// Numbers as a JSON array (NaN → null, as JSON.stringify writes them).
pub fn nums_value(xs: &[f64]) -> Value {
    Value::Array(xs.iter().map(|x| num_value(*x)).collect())
}
/// Points as a JSON array.
pub fn pts_value(pts: &[P]) -> Value {
    Value::Array(pts.iter().map(|p| nums_value(p)).collect())
}

/// `applyGrip(m0, grip, p, {shift})`.
pub fn apply_grip(m0: &Markup, grip: &Grip, p: P, shift: bool) -> GripPatch {
    if grip.kind == "vertex" || grip.kind == "tip" || grip.kind == "knee" {
        let mut pts: Vec<P> = m0.pts().to_vec();
        let gi = grip.i.unwrap_or(0);
        let mut q = p;
        if shift && pts.len() > 1 {
            let nb = if gi > 0 { pts.get(gi.saturating_sub(1)) } else { pts.get(1) };
            if let Some(nb) = nb {
                q = snap45(*nb, p);
            }
        }
        // pts[grip.i] = q: past the end the JS array grows (holes read as null); never the case for real grips
        if gi < pts.len() {
            if let Some(slot) = pts.get_mut(gi) {
                *slot = q;
            }
        } else if gi == pts.len() {
            pts.push(q);
        }
        return GripPatch::Pts(pts);
    }
    let is_box = BOX_TYPES.contains(&m0.kind.as_str());
    let bx = if is_box { m0.rect.unwrap_or([f64::NAN; 4]) } else { points_bounds(m0.pts()) };
    let [mut x0, mut y0, mut x1, mut y1] = bx;
    let k = grip.key.as_str();
    if k.contains('w') {
        x0 = p[0];
    }
    if k.contains('e') {
        x1 = p[0];
    }
    if k.contains('s') {
        y0 = p[1];
    }
    if k.contains('n') {
        y1 = p[1];
    }
    if shift && k.chars().count() == 2 {
        let (w0, h0) = (bx[2] - bx[0], bx[3] - bx[1]);
        if w0 > 0.0 && h0 > 0.0 {
            let sx = (x1 - x0).abs() / w0;
            let sy = (y1 - y0).abs() / h0;
            let sc = max(sx, sy);
            let sg = |d: f64| sign(if d == 0.0 || d.is_nan() { 1.0 } else { d });
            if k.contains('w') {
                x0 = x1 - w0 * sc * sg(x1 - x0);
            } else {
                x1 = x0 + w0 * sc * sg(x1 - x0);
            }
            if k.contains('s') {
                y0 = y1 - h0 * sc * sg(y1 - y0);
            } else {
                y1 = y0 + h0 * sc * sg(y1 - y0);
            }
        }
    }
    let mut nb = norm_rect([x0, y0, x1, y1]);
    let mn = 2.0;
    if nb[2] - nb[0] < mn {
        nb[2] = nb[0] + mn;
    }
    if nb[3] - nb[1] < mn {
        nb[3] = nb[1] + mn;
    }
    if is_box {
        return GripPatch::Rect(nb);
    }
    let bw0 = bx[2] - bx[0];
    let bh0 = bx[3] - bx[1];
    let bw = if bw0 == 0.0 || bw0.is_nan() { 1.0 } else { bw0 };
    let bh = if bh0 == 0.0 || bh0.is_nan() { 1.0 } else { bh0 };
    let horiz = k.contains('w') || k.contains('e');
    let vert = k.contains('n') || k.contains('s');
    GripPatch::Pts(
        m0.pts()
            .iter()
            .map(|q| {
                [
                    if horiz { x0 + ((q[0] - bx[0]) / bw) * (x1 - x0) } else { q[0] },
                    if vert { y0 + ((q[1] - bx[1]) / bh) * (y1 - y0) } else { q[1] },
                ]
            })
            .collect(),
    )
}

/// `moveBy(m, dx, dy)` → the patch (pts, rect, holes as the markup has them).
pub fn move_by(m: &Markup, dx: f64, dy: f64) -> Map<String, Value> {
    let mut patch = Map::new();
    if let Some(p) = &m.pts {
        let v: Vec<P> = p.iter().map(|q| [q[0] + dx, q[1] + dy]).collect();
        patch.insert("pts".into(), pts_value(&v));
    }
    if let Some(r) = &m.rect {
        patch.insert("rect".into(), nums_value(&[r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]));
    }
    if let Some(h) = &m.holes
        && !h.is_empty()
    {
        patch.insert(
            "holes".into(),
            Value::Array(h.iter().map(|ring| pts_value(&ring.iter().map(|q| [q[0] + dx, q[1] + dy]).collect::<Vec<_>>())).collect()),
        );
    }
    patch
}

/// `simplify(pts, tol)` (Douglas–Peucker).
pub fn simplify(pts: &[P], tol: f64) -> Vec<P> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; n];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut stack: Vec<(usize, usize)> = vec![(0, n.saturating_sub(1))];
    while let Some((a, b)) = stack.pop() {
        let mut best: Option<usize> = None;
        let mut bd = tol;
        let (Some(pa), Some(pb)) = (pts.get(a), pts.get(b)) else { continue };
        let (ax, ay, bx, by) = (pa[0], pa[1], pb[0], pb[1]);
        let (dx, dy) = (bx - ax, by - ay);
        let l = hypot(dx, dy);
        for i in a.saturating_add(1)..b {
            let Some(p) = pts.get(i) else { continue };
            let (px, py) = (p[0], p[1]);
            let d = if l != 0.0 && !l.is_nan() { (dy * px - dx * py + bx * ay - by * ax).abs() / l } else { hypot(px - ax, py - ay) };
            if d > bd {
                bd = d;
                best = Some(i);
            }
        }
        if let Some(bi) = best.filter(|b| *b > 0) {
            if let Some(k) = keep.get_mut(bi) {
                *k = true;
            }
            stack.push((a, bi));
            stack.push((bi, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// A markup's display list and bounds (`displayOf`).
#[derive(Debug, Clone)]
pub struct Display {
    pub list: Vec<Item>,
    pub bounds: Option<[f64; 4]>,
}

/// `displayOf(m, env)` (the JS caches it per markup object; here it is computed).
pub fn display_of(m: &Markup, env: &ShapeEnv) -> Display {
    if m.kind == "foreign" {
        return Display { list: Vec::new(), bounds: m.rect };
    }
    let list = shapes(m, env);
    let bounds = list_bounds(&list).or(m.rect);
    Display { list, bounds }
}

/// `boundsOf(m, env)`.
pub fn bounds_of(m: &Markup, env: &ShapeEnv) -> Option<[f64; 4]> {
    if m.kind == "note"
        && let Some(p) = m.pts().first()
    {
        return Some([p[0], p[1] - NOTE_SIZE, p[0] + NOTE_SIZE, p[1]]);
    }
    display_of(m, env).bounds
}

/// `distanceTo(m, p, env)`.
pub fn distance_to(m: &Markup, p: P, env: &ShapeEnv) -> f64 {
    if m.kind == "foreign" {
        let r = m.rect.unwrap_or([f64::NAN; 4]);
        return if p[0] >= r[0] && p[0] <= r[2] && p[1] >= r[1] && p[1] <= r[3] { 0.0 } else { f64::INFINITY };
    }
    if bounds_of(m, env).is_none() {
        return f64::INFINITY;
    }
    list_distance(&display_of(m, env).list, p)
}

/// `pick(list, p, tol, envOf)`: the topmost markup within tol of p (list in drawing order) → its index.
pub fn pick<'a>(list: &[&'a Markup], p: P, tol: f64, env_of: &dyn Fn(&Markup) -> ShapeEnv) -> Option<&'a Markup> {
    for m in list.iter().rev() {
        let env = env_of(m);
        let Some(b) = bounds_of(m, &env) else { continue };
        if p[0] < b[0] - tol || p[0] > b[2] + tol || p[1] < b[1] - tol || p[1] > b[3] + tol {
            continue;
        }
        if distance_to(m, p, &env) <= tol {
            return Some(m);
        }
    }
    None
}

/// `pickWindow(list, r, crossing, envOf)`.
pub fn pick_window<'a>(list: &[&'a Markup], r: [f64; 4], crossing: bool, env_of: &dyn Fn(&Markup) -> ShapeEnv) -> Vec<&'a Markup> {
    let [x0, y0, x1, y1] = r;
    list.iter()
        .copied()
        .filter(|m| {
            let Some(b) = bounds_of(m, &env_of(m)) else { return false };
            if crossing {
                b[0] <= x1 && b[2] >= x0 && b[1] <= y1 && b[3] >= y0
            } else {
                b[0] >= x0 && b[2] <= x1 && b[1] >= y0 && b[3] <= y1
            }
        })
        .collect()
}

/// A 2-D affine transform [a, b, c, d, e, f]: x' = a x + c y + e, y' = b x + d y + f.
pub type Affine = [f64; 6];
pub const I: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `mul(m, n)` = m ∘ n (n first).
pub fn mul(m: &Affine, n: &Affine) -> Affine {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

/// `inv(m)`.
pub fn inv(m: &Affine) -> Affine {
    let det = m[0] * m[3] - m[1] * m[2];
    if det == 0.0 || det.is_nan() {
        return I;
    }
    [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det, (m[2] * m[5] - m[3] * m[4]) / det, (m[1] * m[4] - m[0] * m[5]) / det]
}

/// `apply(m, p)`.
pub fn apply(m: &Affine, p: P) -> P {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

/// `similarity(a1, a2, b1, b2)`: move + turn + uniform scale taking a1, a2 onto b1, b2 (None when either pair is one
/// point).
pub fn similarity(a1: P, a2: P, b1: P, b2: P) -> Option<Affine> {
    let (ax, ay, bx, by) = (a2[0] - a1[0], a2[1] - a1[1], b2[0] - b1[0], b2[1] - b1[1]);
    let (la, lb) = (hypot(ax, ay), hypot(bx, by));
    if la < 1e-9 || lb < 1e-9 {
        return None;
    }
    let s = lb / la;
    let ang = by.jatan2(bx) - ay.jatan2(ax);
    let (c, sn) = (ang.jcos() * s, ang.jsin() * s);
    let e = b1[0] - (c * a1[0] - sn * a1[1]);
    let f = b1[1] - (sn * a1[0] + c * a1[1]);
    Some([c, sn, -sn, c, e, f])
}

/// Two numbers' minimum (re-export for callers).
pub fn min2(a: f64, b: f64) -> f64 {
    min(a, b)
}
