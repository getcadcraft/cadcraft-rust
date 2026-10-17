//! alignment.js — horizontal alignments by PIs with circular curves.
//!
//! The curve facts follow the arc-definition formulas: T = R·tan(Δ/2), L = R·Δ, E = R(sec(Δ/2) − 1),
//! M = R(1 − cos(Δ/2)), chord = 2R·sin(Δ/2). Spirals are not built (as in the JS).

use crate::geo::{P2, p2};
use crate::js::{G, J, JsMath, Obj, hypot, max, min, to_fixed};
use crate::tr;
use serde_json::Value;
use std::f64::consts::PI;

const TAU: f64 = PI * 2.0;
/// `normAng(a)` into (−π, π].
pub fn norm_ang(a: f64) -> f64 {
    let mut a = a % TAU;
    if a <= -PI {
        a += TAU;
    }
    if a > PI {
        a -= TAU;
    }
    a
}

/// A PI `{x, y, r}` (r: radius, 0 / missing = an angle point; kept as the JS value: NaN when not a number).
#[derive(Clone, Copy, Debug, Default)]
pub struct Pi {
    pub x: f64,
    pub y: f64,
    /// `Number(r) || 0` is what the builder uses
    pub r: f64,
}
impl J for Pi {
    fn j(&self) -> Value {
        crate::jo! { "x" => self.x, "y" => self.y, "r" => self.r }
    }
}
/// A PI from JSON: `r` as `Number(q.r) || 0` reads it.
pub fn read_pi(v: &Value) -> Pi {
    Pi { x: crate::js::numf(v.get("x")), y: crate::js::numf(v.get("y")), r: crate::js::number_or0(v.get("r")) }
}

/// One element: a tangent line or a circular arc.
#[derive(Clone, Debug)]
pub enum Element {
    Line { p0: P2, p1: P2, dir: f64, sta: f64, len: f64 },
    Arc(Arc),
}
#[derive(Clone, Debug)]
pub struct Arc {
    pub p0: P2,
    pub p1: P2,
    pub c: P2,
    pub r: f64,
    pub ccw: bool,
    pub delta: f64,
    pub a0: f64,
    pub sta: f64,
    pub len: f64,
    pub pi: P2,
    pub pi_index: usize,
    pub t: f64,
    pub e: f64,
    pub m: f64,
    pub chord: f64,
    pub dir_in: f64,
    pub dir_out: f64,
}
impl Element {
    pub fn sta(&self) -> f64 {
        match self {
            Element::Line { sta, .. } => *sta,
            Element::Arc(a) => a.sta,
        }
    }
    pub fn len(&self) -> f64 {
        match self {
            Element::Line { len, .. } => *len,
            Element::Arc(a) => a.len,
        }
    }
    pub fn p0(&self) -> P2 {
        match self {
            Element::Line { p0, .. } => *p0,
            Element::Arc(a) => a.p0,
        }
    }
    pub fn p1(&self) -> P2 {
        match self {
            Element::Line { p1, .. } => *p1,
            Element::Arc(a) => a.p1,
        }
    }
    pub fn is_arc(&self) -> bool {
        matches!(self, Element::Arc(_))
    }
}
impl J for Element {
    fn j(&self) -> Value {
        match self {
            Element::Line { p0, p1, dir, sta, len } => {
                crate::jo! { "type" => "line", "p0" => *p0, "p1" => *p1, "dir" => *dir, "sta" => *sta, "len" => *len }
            }
            Element::Arc(a) => Obj::new()
                .set("type", "arc")
                .set("p0", a.p0)
                .set("p1", a.p1)
                .set("c", a.c)
                .set("r", a.r)
                .set("ccw", a.ccw)
                .set("delta", a.delta)
                .set("a0", a.a0)
                .set("sta", a.sta)
                .set("len", a.len)
                .set("pi", a.pi)
                .set("piIndex", a.pi_index)
                .set("T", a.t)
                .set("E", a.e)
                .set("M", a.m)
                .set("chord", a.chord)
                .set("dirIn", a.dir_in)
                .set("dirOut", a.dir_out)
                .done(),
        }
    }
}

/// A point on the alignment with its forward direction.
#[derive(Clone, Copy, Debug)]
pub struct At {
    pub x: f64,
    pub y: f64,
    pub dir: f64,
}
impl J for At {
    fn j(&self) -> Value {
        crate::jo! { "x" => self.x, "y" => self.y, "dir" => self.dir }
    }
}
/// `stationOffset(x, y)`.
#[derive(Clone, Copy, Debug)]
pub struct StaOff {
    pub sta: f64,
    pub off: f64,
    pub x: f64,
    pub y: f64,
    pub outside: bool,
    pub dist: f64,
}
impl J for StaOff {
    fn j(&self) -> Value {
        crate::jo! { "sta" => self.sta, "off" => self.off, "x" => self.x, "y" => self.y, "outside" => self.outside, "dist" => self.dist }
    }
}

/// An alignment.
#[derive(Clone, Debug)]
pub struct Alignment {
    pub elements: Vec<Element>,
    pub start: f64,
    pub errors: Vec<String>,
    pub pis: Vec<Pi>,
    pub length: f64,
    pub end: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Curve {
    r: f64,
    d: f64,
    t: f64,
    #[allow(dead_code)]
    a1: f64,
    a2: f64,
}

/// `buildAlignment({ pis, start })`.
pub fn build_alignment(pis: &[Pi], start: f64) -> Alignment {
    let mut errors = Vec::new();
    let mut elements = Vec::new();
    let p: Vec<Pi> = pis.iter().filter(|p| p.x.is_finite() && p.y.is_finite()).copied().collect();
    // drop repeated PIs
    let q: Vec<Pi> = p
        .iter()
        .enumerate()
        .filter(|(i, pp)| *i == 0 || p.get(i.wrapping_sub(1)).is_some_and(|b| hypot(pp.x - b.x, pp.y - b.y) > 1e-9))
        .map(|(_, pp)| *pp)
        .collect();
    if q.len() < 2 {
        return Alignment::new(Vec::new(), start, vec![tr!("al.err.two")], Vec::new());
    }
    let n = q.len();
    let mut curves: Vec<Option<Curve>> = vec![None; n];
    for i in 1..n.saturating_sub(1) {
        let (a, b, c) = (q.g(i - 1), q.g(i), q.g(i + 1));
        let r = if b.r != 0.0 && !b.r.is_nan() { b.r } else { 0.0 };
        let a1 = (b.y - a.y).jatan2(b.x - a.x);
        let a2 = (c.y - b.y).jatan2(c.x - b.x);
        let d = norm_ang(a2 - a1);
        if r <= 0.0 || d.abs() < 1e-9 {
            curves.s(i, Some(Curve { r: 0.0, d, t: 0.0, a1, a2 }));
            continue;
        }
        if (d.abs() - PI).abs() < 1e-6 {
            errors.push(tr!("al.err.back", pi = i + 1));
            curves.s(i, Some(Curve { r: 0.0, d, t: 0.0, a1, a2 }));
            continue;
        }
        curves.s(i, Some(Curve { r, d, t: r * (d.abs() / 2.0).jtan(), a1, a2 }));
    }
    // tangents must fit
    for i in 0..n - 1 {
        let (a, b) = (q.g(i), q.g(i + 1));
        let l = hypot(b.x - a.x, b.y - a.y);
        let tt = |c: Option<Curve>| c.map_or(0.0, |c| if c.t != 0.0 && !c.t.is_nan() { c.t } else { 0.0 });
        let need = tt(curves.g(i)) + tt(curves.g(i + 1));
        if need > l + 1e-9 {
            errors.push(tr!("al.err.overlap", a = i + 1, b = i + 2, need = to_fixed(need, 2), dist = to_fixed(l, 2)));
        }
    }
    if !errors.is_empty() {
        return Alignment::new(Vec::new(), start, errors, q);
    }

    let mut sta = start;
    let mut cur = p2(q.g(0).x, q.g(0).y);
    for i in 1..n {
        let c = if i < n - 1 { curves.g(i) } else { None };
        let (a, b) = (q.g(i - 1), q.g(i));
        let dir = (b.y - a.y).jatan2(b.x - a.x);
        let ux = dir.jcos();
        let uy = dir.jsin();
        let t = c.map_or(0.0, |c| if c.t != 0.0 && !c.t.is_nan() { c.t } else { 0.0 });
        let end = p2(b.x - ux * t, b.y - uy * t); // PC (or the PI / last point)
        let len = hypot(end.x - cur.x, end.y - cur.y);
        if len > 1e-9 {
            elements.push(Element::Line { p0: cur, p1: end, dir, sta, len });
            sta += len;
        }
        match c {
            Some(c) if c.r > 0.0 => {
                let ccw = c.d > 0.0;
                let r = c.r;
                let delta = c.d.abs();
                let nx = if ccw { -uy } else { uy };
                let ny = if ccw { ux } else { -ux }; // toward the centre
                let ctr = p2(end.x + nx * r, end.y + ny * r);
                let dir2 = c.a2;
                let pt = p2(b.x + dir2.jcos() * t, b.y + dir2.jsin() * t);
                let a0 = (end.y - ctr.y).jatan2(end.x - ctr.x);
                let l = r * delta;
                elements.push(Element::Arc(Arc {
                    p0: end,
                    p1: pt,
                    c: ctr,
                    r,
                    ccw,
                    delta,
                    a0,
                    sta,
                    len: l,
                    pi: p2(b.x, b.y),
                    pi_index: i,
                    t,
                    e: r * (1.0 / (delta / 2.0).jcos() - 1.0),
                    m: r * (1.0 - (delta / 2.0).jcos()),
                    chord: 2.0 * r * (delta / 2.0).jsin(),
                    dir_in: dir,
                    dir_out: dir2,
                }));
                sta += l;
                cur = pt;
            }
            _ => cur = p2(b.x, b.y),
        }
    }
    Alignment::new(elements, start, Vec::new(), q)
}

impl Alignment {
    pub fn new(elements: Vec<Element>, start: f64, errors: Vec<String>, pis: Vec<Pi>) -> Alignment {
        let length = elements.iter().fold(0.0, |s, e| s + e.len());
        Alignment { elements, start, errors, pis, length, end: start + length }
    }
    pub fn ok(&self) -> bool {
        !self.elements.is_empty() && self.errors.is_empty()
    }

    pub fn element_at(&self, sta: f64) -> Option<&Element> {
        let els = &self.elements;
        for e in els {
            if sta <= e.sta() + e.len() + 1e-9 {
                return Some(e);
            }
        }
        els.last()
    }

    pub fn point_at(&self, sta: f64) -> Option<At> {
        let e = self.element_at(sta)?;
        let s = sta - e.sta();
        Some(match e {
            Element::Line { p0, dir, .. } => At { x: p0.x + dir.jcos() * s, y: p0.y + dir.jsin() * s, dir: *dir },
            Element::Arc(a) => {
                let sw = (if a.ccw { 1.0 } else { -1.0 }) * s / a.r;
                let ang = a.a0 + sw;
                At { x: a.c.x + ang.jcos() * a.r, y: a.c.y + ang.jsin() * a.r, dir: ang + if a.ccw { PI / 2.0 } else { -PI / 2.0 } }
            }
        })
    }

    /// Nearest station and offset (right = +). `outside` when the point is beyond either end.
    pub fn station_offset(&self, x: f64, y: f64) -> Option<StaOff> {
        struct Best {
            d: f64,
            sta: f64,
            off: f64,
            x: f64,
            y: f64,
        }
        let mut best: Option<Best> = None;
        for e in &self.elements {
            let (s, px, py, dir);
            match e {
                Element::Line { p0, dir: d, len, .. } => {
                    let ux = d.jcos();
                    let uy = d.jsin();
                    s = max(0.0, min(*len, (x - p0.x) * ux + (y - p0.y) * uy));
                    px = p0.x + ux * s;
                    py = p0.y + uy * s;
                    dir = *d;
                }
                Element::Arc(a) => {
                    let ang = (y - a.c.y).jatan2(x - a.c.x);
                    let mut sw = ((ang - a.a0) * if a.ccw { 1.0 } else { -1.0 }) % TAU;
                    if sw < 0.0 {
                        sw += TAU;
                    }
                    if sw > a.delta {
                        sw = if sw - a.delta < TAU - sw { a.delta } else { 0.0 }; // off the arc: its nearer end
                    }
                    s = sw * a.r;
                    let aa = a.a0 + if a.ccw { sw } else { -sw };
                    px = a.c.x + aa.jcos() * a.r;
                    py = a.c.y + aa.jsin() * a.r;
                    dir = aa + if a.ccw { PI / 2.0 } else { -PI / 2.0 };
                }
            }
            let d = hypot(x - px, y - py);
            if best.as_ref().is_none_or(|b| d < b.d - 1e-12) {
                let off = (x - px) * dir.jsin() - (y - py) * dir.jcos(); // + to the right
                best = Some(Best { d, sta: e.sta() + s, off, x: px, y: py });
            }
        }
        let best = best?;
        // beyond the ends: the projection is not perpendicular
        let along = |p: &At| (x - p.x) * p.dir.jcos() + (y - p.y) * p.dir.jsin();
        let p0 = self.point_at(self.start)?;
        let p1 = self.point_at(self.end)?;
        let outside = (best.sta <= self.start + 1e-9 && along(&p0) < -1e-9) || (best.sta >= self.end - 1e-9 && along(&p1) > 1e-9);
        Some(StaOff { sta: best.sta, off: best.off, x: best.x, y: best.y, outside, dist: best.d })
    }

    /// `polyline(maxSeg, maxTurn)` → [{x, y, sta}].
    pub fn polyline(&self, max_seg: f64, max_turn: f64) -> Vec<(f64, f64, f64)> {
        let mut out = Vec::new();
        for e in &self.elements {
            if out.is_empty() {
                let p = e.p0();
                out.push((p.x, p.y, e.sta()));
            }
            match e {
                Element::Line { p1, sta, len, .. } => out.push((p1.x, p1.y, sta + len)),
                Element::Arc(a) => {
                    let n = max(max(1.0, (a.delta / max_turn).ceil()), if max_seg.is_finite() { (a.len / max_seg).ceil() } else { 1.0 });
                    let mut k = 1.0;
                    while k <= n {
                        let s = a.sta + (a.len * k) / n;
                        if let Some(p) = self.point_at(s) {
                            out.push((p.x, p.y, s));
                        }
                        k += 1.0;
                    }
                }
            }
        }
        out
    }

    /// `ticks(step)` → [{sta, x, y, dir}] at every whole multiple of step.
    pub fn ticks(&self, step: f64) -> Vec<(f64, At)> {
        if !(step > 0.0) || self.elements.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let k0 = (self.start / step - 1e-9).ceil();
        let k1 = (self.end / step + 1e-9).floor();
        if k1 - k0 > 20000.0 {
            return Vec::new();
        }
        let mut k = k0;
        while k <= k1 {
            let sta = k * step;
            if let Some(p) = self.point_at(sta) {
                out.push((sta, p));
            }
            k += 1.0;
        }
        out
    }

    /// Key stations: start, PCs, PTs, end.
    pub fn key_stations(&self) -> Vec<(f64, &'static str)> {
        let mut out = vec![(self.start, "Start")];
        for e in &self.elements {
            if let Element::Arc(a) = e {
                out.push((a.sta, "PC"));
                out.push((a.sta + a.len, "PT"));
            }
        }
        out.push((self.end, "End"));
        out
    }
}

impl J for Alignment {
    fn j(&self) -> Value {
        crate::jo! {
            "elements" => self.elements, "start" => self.start, "errors" => self.errors, "pis" => self.pis,
            "length" => self.length, "end" => self.end, "ok" => self.ok(),
        }
    }
}

crate::und_default!(Pi, Curve);
